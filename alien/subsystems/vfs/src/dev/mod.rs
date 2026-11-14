use alloc::{collections::BTreeMap, sync::Arc};

use constants::{
    io::{TeletypeCommand, Termios, WinSize},
    DeviceId,
};
use devfs::DevKernelProvider;
use devices::{
    BLKDevice, GPUDevice, INPUTDevice, RTCDevice, UARTDevice, BLOCK_DEVICE, GPU_DEVICE,
    KEYBOARD_INPUT_DEVICE, MOUSE_INPUT_DEVICE, RTC_DEVICE, UART_DEVICE,
};
use ksync::Mutex;
use log::info;
use null::NullDevice;
use random::RandomDevice;
use spin::Lazy;
use vfscore::{
    dentry::VfsDentry,
    error::VfsError,
    file::VfsFile,
    fstype::VfsFsType,
    inode::{InodeAttr, VfsInode},
    superblock::VfsSuperBlock,
    utils::{VfsFileStat, VfsNodeType, VfsTimeSpec},
    VfsResult,
};

mod null;
mod random;

pub static DEVICES: Lazy<Mutex<BTreeMap<DeviceId, Arc<dyn VfsInode>>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

pub static DEVICE_ID_MANAGER: Lazy<Mutex<DeviceIdManager>> =
    Lazy::new(|| Mutex::new(DeviceIdManager::new()));

pub fn register_device(inode: Arc<dyn VfsInode>) {
    let rdev = inode.get_attr().unwrap().st_rdev;
    let device_id = DeviceId::from(rdev);
    DEVICES.lock().insert(device_id, inode);
}

pub fn unregister_device(rdev: DeviceId) {
    DEVICES.lock().remove(&rdev);
}

pub fn alloc_device_id(inode_type: VfsNodeType) -> DeviceId {
    DEVICE_ID_MANAGER.lock().alloc(inode_type)
}

pub trait InodeType2u32 {
    fn to_u32(&self) -> u32;
}

impl InodeType2u32 for VfsNodeType {
    fn to_u32(&self) -> u32 {
        match self {
            VfsNodeType::CharDevice => 2,
            VfsNodeType::BlockDevice => 3,
            _ => 0,
        }
    }
}

pub struct DeviceIdManager {
    map: BTreeMap<u32, u32>,
}

impl DeviceIdManager {
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }
    pub fn alloc(&mut self, inode_type: VfsNodeType) -> DeviceId {
        assert!(matches!(
            inode_type,
            VfsNodeType::CharDevice | VfsNodeType::BlockDevice
        ));
        let id = self.map.entry(inode_type.to_u32()).or_insert(0);
        *id += 1;
        DeviceId::new(inode_type.to_u32(), *id)
    }
}

/// 在没有 UART 驱动时，用 SBI 控制台提供 /dev/tty。
struct SbiConsoleDevice {
    device_id: DeviceId,
    io: Mutex<IoData>,
}

#[derive(Default)]
struct IoData {
    foreground_pgid: u32,
    winsize: WinSize,
    termios: Termios,
}

impl SbiConsoleDevice {
    fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            io: Mutex::new(IoData::default()),
        }
    }

    fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

impl VfsFile for SbiConsoleDevice {
    fn read_at(&self, _offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let ch = platform::console_getchar();
            if ch != '\0' && ch != '\u{ff}' {
                buf[0] = if ch == '\r' { b'\n' } else { ch as u8 };
                return Ok(1);
            }
            core::hint::spin_loop();
        }
    }

    fn write_at(&self, _offset: u64, buf: &[u8]) -> VfsResult<usize> {
        let text = core::str::from_utf8(buf).unwrap_or("");
        platform::console::console_write(text);
        Ok(buf.len())
    }

    fn ioctl(&self, cmd: u32, arg: usize) -> VfsResult<usize> {
        let cmd = TeletypeCommand::try_from(cmd).map_err(|_| VfsError::NoSys)?;
        let mut io = self.io.lock();
        match cmd {
            TeletypeCommand::TCGETS | TeletypeCommand::TCGETA => {
                shim::copy_data_to_task(&io.termios, arg as *mut Termios);
                Ok(0)
            }
            TeletypeCommand::TCSETS
            | TeletypeCommand::TCSETSW
            | TeletypeCommand::TCSETSF => {
                shim::copy_data_from_task(arg as *const Termios, &mut io.termios);
                Ok(0)
            }
            TeletypeCommand::TIOCGPGRP => {
                shim::copy_data_to_task(&io.foreground_pgid, arg as *mut u32);
                Ok(0)
            }
            TeletypeCommand::TIOCSPGRP => {
                shim::copy_data_from_task(arg as *const u32, &mut io.foreground_pgid);
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
            _ => Err(VfsError::NoSys),
        }
    }
}

impl VfsInode for SbiConsoleDevice {
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

#[derive(Clone)]
pub struct DevFsProviderImpl;
impl DevKernelProvider for DevFsProviderImpl {
    fn current_time(&self) -> VfsTimeSpec {
        VfsTimeSpec::new(0, 0)
    }
    fn rdev2device(&self, rdev: u64) -> Option<Arc<dyn VfsInode>> {
        let device_id = DeviceId::from(rdev);
        DEVICES.lock().get(&device_id).cloned()
    }
}

///```bash
/// |
/// |-- null
/// |-- zero
/// |-- random
/// |-- urandom
/// |-- tty
/// |-- shm (a ramfs will be mounted here)
/// |-- misc
///    |-- rtc
/// ```
pub fn init_devfs(devfs: Arc<dyn VfsFsType>) -> Arc<dyn VfsDentry> {
    let root = devfs.i_mount(0, "/dev", None, &[]).unwrap();
    let root_inode = root.inode().unwrap();

    let null_device = Arc::new(NullDevice::new(alloc_device_id(VfsNodeType::CharDevice)));
    let zero_device = Arc::new(NullDevice::new(alloc_device_id(VfsNodeType::CharDevice)));
    let random_device = Arc::new(RandomDevice::new(alloc_device_id(VfsNodeType::CharDevice)));
    let urandom_device = Arc::new(RandomDevice::new(alloc_device_id(VfsNodeType::CharDevice)));

    root_inode
        .create(
            "null",
            'c'.into(),
            "rw-rw-rw-".into(),
            Some(null_device.device_id().id()),
        )
        .unwrap();
    root_inode
        .create(
            "zero",
            'c'.into(),
            "rw-rw-rw-".into(),
            Some(zero_device.device_id().id()),
        )
        .unwrap();
    root_inode
        .create(
            "random",
            'c'.into(),
            "rw-rw-rw-".into(),
            Some(random_device.device_id().id()),
        )
        .unwrap();
    root_inode
        .create(
            "urandom",
            'c'.into(),
            "rw-rw-rw-".into(),
            Some(urandom_device.device_id().id()),
        )
        .unwrap();

    register_device(null_device);
    register_device(zero_device);
    register_device(random_device);
    register_device(urandom_device);

    root_inode
        .create("shm", VfsNodeType::Dir, "rwxrwxrwx".into(), None)
        .unwrap();
    root_inode
        .create("misc", VfsNodeType::Dir, "rwxrwxrwx".into(), None)
        .unwrap();

    scan_system_devices(root_inode);
    // todo!(tty,shm,misc)
    println!("devfs init success");
    root
}

fn scan_system_devices(root: Arc<dyn VfsInode>) {
    BLOCK_DEVICE.get().map(|blk| {
        let block_device = Arc::new(BLKDevice::new(
            alloc_device_id(VfsNodeType::BlockDevice),
            blk.clone(),
        ));
        root.create(
            "sda",
            VfsNodeType::BlockDevice,
            "rw-rw----".into(),
            Some(block_device.device_id().id()),
        )
        .unwrap();
        info!("block device id: {}", block_device.device_id().id());
        register_device(block_device);
    });
    GPU_DEVICE.get().map(|gpu| {
        let gpu_device = Arc::new(GPUDevice::new(
            alloc_device_id(VfsNodeType::CharDevice),
            gpu.clone(),
        ));
        root.create(
            "gpu",
            VfsNodeType::BlockDevice,
            "rw-rw----".into(),
            Some(gpu_device.device_id().id()),
        )
        .unwrap();
        info!("gpu device id: {}", gpu_device.device_id().id());
        register_device(gpu_device);
    });
    KEYBOARD_INPUT_DEVICE.get().map(|input| {
        let input_device = Arc::new(INPUTDevice::new(
            alloc_device_id(VfsNodeType::CharDevice),
            input.clone(),
            false,
        ));
        root.create(
            "keyboard",
            VfsNodeType::BlockDevice,
            "rw-rw----".into(),
            Some(input_device.device_id().id()),
        )
        .unwrap();
        info!("keyboard device id: {}", input_device.device_id().id());
        register_device(input_device);
    });
    MOUSE_INPUT_DEVICE.get().map(|input| {
        let input_device = Arc::new(INPUTDevice::new(
            alloc_device_id(VfsNodeType::CharDevice),
            input.clone(),
            true,
        ));
        root.create(
            "mouse",
            VfsNodeType::BlockDevice,
            "rw-rw----".into(),
            Some(input_device.device_id().id()),
        )
        .unwrap();
        info!("mouse device id: {}", input_device.device_id().id());
        register_device(input_device);
    });
    RTC_DEVICE.get().map(|rtc| {
        let rtc_device = Arc::new(RTCDevice::new(
            alloc_device_id(VfsNodeType::CharDevice),
            rtc.clone(),
        ));
        root.create(
            "rtc",
            VfsNodeType::BlockDevice,
            "rw-rw----".into(),
            Some(rtc_device.device_id().id()),
        )
        .unwrap();
        info!("rtc device id: {}", rtc_device.device_id().id());
        register_device(rtc_device);
    });
    UART_DEVICE.get().map(|uart| {
        let uart_device = Arc::new(UARTDevice::new(
            alloc_device_id(VfsNodeType::CharDevice),
            uart.clone(),
        ));
        root.create(
            "tty",
            VfsNodeType::BlockDevice,
            "rw-rw----".into(),
            Some(uart_device.device_id().id()),
        )
        .unwrap();
        info!("uart device id: {}", uart_device.device_id().id());
        register_device(uart_device);
    });
    if UART_DEVICE.get().is_none() {
        let sbi_console =
            Arc::new(SbiConsoleDevice::new(alloc_device_id(VfsNodeType::CharDevice)));
        root.create(
            "tty",
            VfsNodeType::CharDevice,
            "rw-rw-rw-".into(),
            Some(sbi_console.device_id().id()),
        )
        .unwrap();
        register_device(sbi_console);
        info!("sbi console device registered");
    }
}
