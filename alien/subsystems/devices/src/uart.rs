use alloc::{boxed::Box, sync::Arc};

use constants::{
    io::{LocalModes, TeletypeCommand, Termios, WinSize},
    DeviceId,
};
use device_interface::{DeviceBase, UartDevice};
use drivers::uart::LowUartDriver;
use ksync::Mutex;
use spin::Once;
use vfscore::{
    error::VfsError,
    file::VfsFile,
    inode::{InodeAttr, VfsInode},
    superblock::VfsSuperBlock,
    utils::{VfsFileStat, VfsNodeType, VfsPollEvents},
    VfsResult,
};

pub static UART_DEVICE: Once<Arc<dyn UartDevice>> = Once::new();

pub fn init_uart(uart: Arc<dyn UartDevice>) {
    UART_DEVICE.call_once(|| uart);
}

/// 轮询模式的 UART。适用于没有接入中断控制器的平台。
///
/// `get` 会阻塞直到读到字节，避免调用方处理 `None`。
pub struct PollingUart {
    inner: Mutex<PollingInner>,
}

struct PollingInner {
    raw: Box<dyn LowUartDriver>,
    pending: Option<u8>,
}

impl PollingUart {
    pub fn new(raw: Box<dyn LowUartDriver>) -> Self {
        Self {
            inner: Mutex::new(PollingInner {
                raw,
                pending: None,
            }),
        }
    }
}

impl UartDevice for PollingUart {
    fn put(&self, c: u8) {
        self.inner.lock().raw._put(c);
    }

    fn get(&self) -> Option<u8> {
        loop {
            let mut inner = self.inner.lock();
            if let Some(c) = inner.pending.take() {
                return Some(c);
            }
            if let Some(c) = inner.raw._read() {
                return Some(c);
            }
            drop(inner);
            core::hint::spin_loop();
        }
    }

    fn put_bytes(&self, bytes: &[u8]) {
        for &c in bytes {
            if c == b'\n' {
                self.put(b'\r');
            }
            self.put(c);
        }
    }

    fn have_data_to_get(&self) -> bool {
        let mut inner = self.inner.lock();
        if inner.pending.is_some() {
            return true;
        }
        if let Some(c) = inner.raw._read() {
            inner.pending = Some(c);
            true
        } else {
            false
        }
    }

    fn have_space_to_put(&self) -> bool {
        true
    }
}

impl DeviceBase for PollingUart {
    fn handle_irq(&self) {}
}

#[derive(Debug, Default)]
pub struct IoData {
    foreground_pgid: u32,
    winsize: WinSize,
    termios: Termios,
}

pub struct UARTDevice {
    device_id: DeviceId,
    device: Arc<dyn UartDevice>,
    io: Mutex<IoData>,
}

impl UARTDevice {
    pub fn new(device_id: DeviceId, device: Arc<dyn UartDevice>) -> Self {
        Self {
            device_id,
            device,
            io: Mutex::new(IoData::default()),
        }
    }
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// 在控制台输出锁内写入单个字节。
    fn put_locked(&self, ch: u8) {
        platform::console::with_console_lock(|| self.device.put(ch));
    }

    /// 在控制台输出锁内写入一段字节。
    fn put_bytes_locked(&self, buf: &[u8]) {
        platform::console::with_console_lock(|| self.device.put_bytes(buf));
    }
}

impl VfsFile for UARTDevice {
    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        // read util \r and transform to \n
        let mut read_count = 0;
        loop {
            let ch = self.device.get();
            assert!(ch.is_some());
            let ch = ch.unwrap();
            buf[read_count] = ch;
            read_count += 1;
            if ch == b'\r' {
                buf[read_count - 1] = b'\n';
                if LocalModes::from_bits_truncate(self.io.lock().termios.lflag)
                    .contains(LocalModes::ECHO)
                {
                    self.put_locked(b'\n');
                }
                break;
            }
            if LocalModes::from_bits_truncate(self.io.lock().termios.lflag)
                .contains(LocalModes::ECHO)
            {
                self.put_locked(ch);
            }
            if read_count >= buf.len() {
                break;
            }
        }
        Ok(read_count)
    }
    fn write_at(&self, _offset: u64, buf: &[u8]) -> VfsResult<usize> {
        self.put_bytes_locked(buf);
        Ok(buf.len())
    }
    fn poll(&self, event: VfsPollEvents) -> VfsResult<VfsPollEvents> {
        let mut res = VfsPollEvents::empty();
        if event.contains(VfsPollEvents::IN) {
            if self.device.have_data_to_get() {
                res |= VfsPollEvents::IN;
            }
        }
        if event.contains(VfsPollEvents::OUT) {
            if self.device.have_space_to_put() {
                res |= VfsPollEvents::OUT
            }
        }
        Ok(res)
    }
    fn ioctl(&self, cmd: u32, arg: usize) -> VfsResult<usize> {
        let mut io = self.io.lock();
        let cmd = TeletypeCommand::try_from(cmd).map_err(|_| VfsError::Invalid)?;
        return match cmd {
            TeletypeCommand::TCGETS | TeletypeCommand::TCGETA => {
                shim::copy_data_to_task(&io.termios, arg as *mut Termios);
                Ok(0)
            }
            TeletypeCommand::TCSETS | TeletypeCommand::TCSETSW | TeletypeCommand::TCSETSF => {
                shim::copy_data_from_task(arg as *const Termios, &mut io.termios);
                Ok(0)
            }
            TeletypeCommand::TIOCGPGRP => {
                let word = shim::transfer_ptr_mut(arg as *mut u32);
                *word = io.foreground_pgid;
                Ok(0)
            }
            TeletypeCommand::TIOCSPGRP => {
                let word = shim::transfer_ptr(arg as *const u32);
                io.foreground_pgid = *word;
                Ok(0)
            }
            TeletypeCommand::TIOCGWINSZ => {
                shim::copy_data_to_task(&io.winsize, arg as *mut WinSize);
                Ok(0)
            }
            TeletypeCommand::TIOCSWINSZ => {
                shim::copy_data_from_task(arg as *const WinSize, &mut io.winsize);
                Ok(0)
            }
            _ => {
                Err(VfsError::NoSys)
            }
        };
    }
    fn flush(&self) -> VfsResult<()> {
        Ok(())
    }
    fn fsync(&self) -> VfsResult<()> {
        Ok(())
    }
}

impl VfsInode for UARTDevice {
    fn get_super_block(&self) -> VfsResult<Arc<dyn VfsSuperBlock>> {
        Err(VfsError::NoSys)
    }

    fn set_attr(&self, _attr: InodeAttr) -> VfsResult<()> {
        Ok(())
    }

    fn get_attr(&self) -> VfsResult<VfsFileStat> {
        Ok(VfsFileStat {
            st_rdev: self.device_id.id(),
            ..Default::default()
        })
    }

    fn inode_type(&self) -> VfsNodeType {
        VfsNodeType::CharDevice
    }
}
