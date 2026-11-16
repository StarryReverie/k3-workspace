//! 最小 NVMe 轮询驱动。
//!
//! 阶段 9A 覆盖控制器复位、管理队列、Identify、I/O 队列与块读取。
//! 所有队列与数据页使用内核恒等映射，地址可直接作为物理地址使用。

use core::{
    hint::spin_loop,
    ptr::{read_volatile, write_volatile},
    sync::atomic::{fence, Ordering},
};

use mem::{alloc_frames, free_frames};
use platform::{config::CLOCK_FREQ, println};
use timer::read_timer;

const ADMIN_QUEUE_SIZE: u16 = 32;
const IO_QUEUE_SIZE: u16 = 32;
const IO_QID: u16 = 1;
const REG_CAP: usize = 0x00;
const REG_INTMS: usize = 0x0c;
const REG_CC: usize = 0x14;
const REG_CSTS: usize = 0x1c;
const REG_AQA: usize = 0x24;
const REG_ASQ: usize = 0x28;
const REG_ACQ: usize = 0x30;
const REG_DOORBELL_BASE: usize = 0x1000;

const CC_EN: u32 = 1;
const CC_IOSQES: u32 = 6 << 16;
const CC_IOCQES: u32 = 4 << 20;
const CSTS_RDY: u32 = 1;

/// NVMe 驱动的最小结果类型。
pub type NvmeResult<T> = Result<T, &'static str>;

/// 一页 DMA 缓冲，按恒等映射地址访问。
struct DmaPage {
    addr: usize,
}

impl DmaPage {
    fn new() -> Self {
        let addr = alloc_frames(1) as usize;
        let page = Self { addr };
        page.zero();
        page
    }

    fn addr(&self) -> usize {
        self.addr
    }

    fn zero(&self) {
        unsafe {
            core::ptr::write_bytes(self.addr as *mut u8, 0, config::FRAME_SIZE);
        }
    }
}

impl Drop for DmaPage {
    fn drop(&mut self) {
        free_frames(self.addr as *mut u8, 1);
    }
}

/// 64 字节的 NVMe 提交队列条目。
#[repr(C)]
#[derive(Clone, Copy)]
struct NvmeCommand {
    dwords: [u32; 16],
}

impl NvmeCommand {
    fn new(opcode: u8) -> Self {
        let mut command = Self { dwords: [0; 16] };
        command.dwords[0] = opcode as u32;
        command
    }

    fn set_cid(&mut self, cid: u16) {
        self.dwords[0] = (self.dwords[0] & 0x0000_ffff) | ((cid as u32) << 16);
    }

    fn set_nsid(&mut self, nsid: u32) {
        self.dwords[1] = nsid;
    }

    fn set_prp1(&mut self, address: u64) {
        // PRP1 位于提交队列条目的字节 24–31，即 dword 6–7。
        self.dwords[6] = address as u32;
        self.dwords[7] = (address >> 32) as u32;
    }

    fn set_prp2(&mut self, address: u64) {
        // PRP2 位于字节 32–39，即 dword 8–9。
        self.dwords[8] = address as u32;
        self.dwords[9] = (address >> 32) as u32;
    }

    fn set_cdw10(&mut self, value: u32) {
        self.dwords[10] = value;
    }

    fn set_cdw11(&mut self, value: u32) {
        self.dwords[11] = value;
    }

    fn set_cdw12(&mut self, value: u32) {
        self.dwords[12] = value;
    }
}

/// Identify 命令的结果。
pub struct NvmeIdentify {
    model: [u8; 40],
    serial: [u8; 20],
    firmware: [u8; 8],
    pub nsze: u64,
    pub lba_size: u32,
}

impl NvmeIdentify {
    pub fn print(&self) {
        println!(
            "NVMe: model={} serial={} fw={}",
            ascii(&self.model),
            ascii(&self.serial),
            ascii(&self.firmware)
        );
        println!(
            "NVMe: namespace 1 blocks={} lba={}",
            self.nsze, self.lba_size
        );
    }
}

fn ascii(bytes: &[u8]) -> &str {
    let end = bytes
        .iter()
        .rposition(|&byte| byte != b' ' && byte != 0)
        .map(|index| index + 1)
        .unwrap_or(0);
    core::str::from_utf8(&bytes[..end]).unwrap_or("?")
}

/// NVMe 控制器。
pub struct NvmeController {
    bar: usize,
    doorbell_stride: usize,
    admin_sq: DmaPage,
    admin_cq: DmaPage,
    admin_sq_tail: u16,
    admin_cq_head: u16,
    admin_phase: bool,
    io_sq: DmaPage,
    io_cq: DmaPage,
    io_sq_tail: u16,
    io_cq_head: u16,
    io_phase: bool,
    next_cid: u16,
    nsid: u32,
    block_size: u32,
}

impl NvmeController {
    pub fn new(bar: usize) -> NvmeResult<Self> {
        let cap = read64(bar, REG_CAP);
        let doorbell_stride = 4usize << ((cap >> 32) & 0xf) as usize;

        write32(bar, REG_INTMS, u32::MAX);
        write32(bar, REG_CC, 0);
        if !wait_ready(bar, false) {
            println!("NVMe: CSTS={:#x} while stopping", read32(bar, REG_CSTS));
            return Err("controller did not stop");
        }

        let admin_sq = DmaPage::new();
        let admin_cq = DmaPage::new();
        write64(bar, REG_ASQ, admin_sq.addr() as u64);
        write64(bar, REG_ACQ, admin_cq.addr() as u64);
        write32(
            bar,
            REG_AQA,
            (((ADMIN_QUEUE_SIZE - 1) as u32) << 16) | (ADMIN_QUEUE_SIZE - 1) as u32,
        );
        write32(bar, REG_CC, CC_IOSQES | CC_IOCQES | CC_EN);
        if !wait_ready(bar, true) {
            println!("NVMe: CSTS={:#x} while starting", read32(bar, REG_CSTS));
            return Err("controller did not start");
        }
        println!("NVMe: controller ready, cap={:#x}", cap);

        let mut controller = Self {
            bar,
            doorbell_stride,
            admin_sq,
            admin_cq,
            admin_sq_tail: 0,
            admin_cq_head: 0,
            admin_phase: true,
            io_sq: DmaPage::new(),
            io_cq: DmaPage::new(),
            io_sq_tail: 0,
            io_cq_head: 0,
            io_phase: true,
            next_cid: 1,
            nsid: 1,
            block_size: 512,
        };
        controller.setup_io_queues()?;
        Ok(controller)
    }

    pub fn identify(&mut self) -> NvmeResult<NvmeIdentify> {
        let page = DmaPage::new();

        let mut command = NvmeCommand::new(0x06);
        command.set_prp1(page.addr() as u64);
        command.set_cdw10(0x01);
        let _ = self.submit_admin(&mut command)?;

        let data =
            unsafe { core::slice::from_raw_parts(page.addr() as *const u8, config::FRAME_SIZE) };
        let mut model = [0u8; 40];
        let mut serial = [0u8; 20];
        let mut firmware = [0u8; 8];
        model.copy_from_slice(&data[24..64]);
        serial.copy_from_slice(&data[4..24]);
        firmware.copy_from_slice(&data[64..72]);

        page.zero();
        let mut command = NvmeCommand::new(0x06);
        command.set_nsid(1);
        command.set_prp1(page.addr() as u64);
        command.set_cdw10(0x00);
        let _ = self.submit_admin(&mut command)?;

        let data =
            unsafe { core::slice::from_raw_parts(page.addr() as *const u8, config::FRAME_SIZE) };
        let nsze = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let flbas = (data[26] & 0x0f) as usize;
        let offset = 128 + flbas * 4;
        let lbaf = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        let lba_size = 1u32 << ((lbaf >> 16) & 0xf);

        self.nsid = 1;
        self.block_size = lba_size;
        Ok(NvmeIdentify {
            model,
            serial,
            firmware,
            nsze,
            lba_size,
        })
    }

    /// 从指定 LBA 读取若干块。
    pub fn read_blocks(&mut self, start_lba: u64, buf: &mut [u8]) -> NvmeResult<()> {
        if buf.is_empty() {
            return Ok(());
        }
        let block_size = self.block_size as usize;
        if block_size == 0 || buf.len() % block_size != 0 {
            return Err("buffer is not block aligned");
        }
        let blocks = buf.len() / block_size;
        if blocks > 0x10000 {
            return Err("too many blocks");
        }

        let list = DmaPage::new();
        let (prp1, prp2) = prp_for(buf, &list);
        let mut command = NvmeCommand::new(0x02);
        command.set_nsid(self.nsid);
        command.set_prp1(prp1);
        if prp2 != 0 {
            command.set_prp2(prp2);
        }
        command.set_cdw10(start_lba as u32);
        command.set_cdw11((start_lba >> 32) as u32);
        command.set_cdw12((blocks as u32 - 1) & 0xffff);
        self.submit_io(&mut command)
    }

    fn setup_io_queues(&mut self) -> NvmeResult<()> {
        let mut command = NvmeCommand::new(0x09);
        command.set_cdw10(0x07);
        command.set_cdw11(0);
        let result = self.submit_admin(&mut command)?;
        println!("NVMe: queue result={:#x}", result);

        let mut command = NvmeCommand::new(0x05);
        command.set_prp1(self.io_cq.addr() as u64);
        command.set_cdw10((((IO_QUEUE_SIZE - 1) as u32) << 16) | IO_QID as u32);
        command.set_cdw11(1);
        let _ = self.submit_admin(&mut command)?;

        let mut command = NvmeCommand::new(0x01);
        command.set_prp1(self.io_sq.addr() as u64);
        command.set_cdw10((((IO_QUEUE_SIZE - 1) as u32) << 16) | IO_QID as u32);
        command.set_cdw11(1 | ((IO_QID as u32) << 16));
        let _ = self.submit_admin(&mut command)?;

        println!("NVMe: I/O queue ready");
        Ok(())
    }

    fn submit_admin(&mut self, command: &mut NvmeCommand) -> NvmeResult<u32> {
        let cid = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1);
        command.set_cid(cid);

        let slot = self.admin_sq_tail as usize * 64;
        unsafe {
            core::ptr::copy_nonoverlapping(
                command as *const NvmeCommand as *const u8,
                (self.admin_sq.addr() + slot) as *mut u8,
                64,
            );
        }
        self.admin_sq_tail = (self.admin_sq_tail + 1) % ADMIN_QUEUE_SIZE;
        fence(Ordering::SeqCst);
        write32(self.bar, self.sq_doorbell(0), self.admin_sq_tail as u32);

        self.wait_admin(cid)
    }

    fn wait_admin(&mut self, cid: u16) -> NvmeResult<u32> {
        let start = read_timer();
        loop {
            let entry = (self.admin_cq.addr() + self.admin_cq_head as usize * 16) as *const u32;
            let dw3 = unsafe { read_volatile(entry.add(3)) };
            let phase = (dw3 >> 16) & 1 != 0;
            if phase == self.admin_phase {
                let received = (dw3 & 0xffff) as u16;
                let status = (dw3 >> 16) & 0xfffe;
                let result = unsafe { read_volatile(entry) };
                self.admin_cq_head = (self.admin_cq_head + 1) % ADMIN_QUEUE_SIZE;
                if self.admin_cq_head == 0 {
                    self.admin_phase = !self.admin_phase;
                }
                fence(Ordering::SeqCst);
                write32(self.bar, self.cq_doorbell(0), self.admin_cq_head as u32);
                if received != cid {
                    println!("NVMe: completion cid {} != {}", received, cid);
                    return Err("completion cid mismatch");
                }
                if status != 0 {
                    println!("NVMe: admin status={:#x}", status);
                    return Err("admin command failed");
                }
                return Ok(result);
            }
            if read_timer().wrapping_sub(start) >= CLOCK_FREQ {
                return Err("admin command timeout");
            }
            spin_loop();
        }
    }

    fn submit_io(&mut self, command: &mut NvmeCommand) -> NvmeResult<()> {
        let cid = self.next_cid;
        self.next_cid = self.next_cid.wrapping_add(1);
        command.set_cid(cid);

        let slot = self.io_sq_tail as usize * 64;
        unsafe {
            core::ptr::copy_nonoverlapping(
                command as *const NvmeCommand as *const u8,
                (self.io_sq.addr() + slot) as *mut u8,
                64,
            );
        }
        self.io_sq_tail = (self.io_sq_tail + 1) % IO_QUEUE_SIZE;
        fence(Ordering::SeqCst);
        write32(self.bar, self.sq_doorbell(IO_QID), self.io_sq_tail as u32);

        self.wait_io(cid)
    }

    fn wait_io(&mut self, cid: u16) -> NvmeResult<()> {
        let start = read_timer();
        loop {
            let entry = (self.io_cq.addr() + self.io_cq_head as usize * 16) as *const u32;
            let dw3 = unsafe { read_volatile(entry.add(3)) };
            let phase = (dw3 >> 16) & 1 != 0;
            if phase == self.io_phase {
                let received = (dw3 & 0xffff) as u16;
                let status = (dw3 >> 16) & 0xfffe;
                self.io_cq_head = (self.io_cq_head + 1) % IO_QUEUE_SIZE;
                if self.io_cq_head == 0 {
                    self.io_phase = !self.io_phase;
                }
                fence(Ordering::SeqCst);
                write32(self.bar, self.cq_doorbell(IO_QID), self.io_cq_head as u32);
                if received != cid {
                    println!("NVMe: I/O completion cid {} != {}", received, cid);
                    return Err("I/O completion cid mismatch");
                }
                if status != 0 {
                    println!("NVMe: I/O status={:#x}", status);
                    return Err("I/O command failed");
                }
                return Ok(());
            }
            if read_timer().wrapping_sub(start) >= CLOCK_FREQ {
                return Err("I/O command timeout");
            }
            spin_loop();
        }
    }

    fn sq_doorbell(&self, qid: u16) -> usize {
        REG_DOORBELL_BASE + (2 * qid as usize) * self.doorbell_stride
    }

    fn cq_doorbell(&self, qid: u16) -> usize {
        REG_DOORBELL_BASE + (2 * qid as usize + 1) * self.doorbell_stride
    }
}

/// 为一页以上的缓冲区构造 PRP1 与 PRP2。
///
/// 不超过两页时直接返回地址；超过两页时用 `list` 页存放后续页地址。
fn prp_for(buf: &[u8], list: &DmaPage) -> (u64, u64) {
    let start = buf.as_ptr() as usize;
    let offset = start & 0xfff;
    let pages = (offset + buf.len() + 0xfff) / 0x1000;
    if pages <= 1 {
        return (start as u64, 0);
    }
    if pages == 2 {
        return (start as u64, ((start & !0xfff) + 0x1000) as u64);
    }

    let first = start & !0xfff;
    for index in 1..pages {
        let entry = (first + index * 0x1000) as u64;
        unsafe {
            write_volatile((list.addr() as *mut u64).add(index - 1), entry);
        }
    }
    (start as u64, list.addr() as u64)
}

fn wait_ready(bar: usize, ready: bool) -> bool {
    let start = read_timer();
    loop {
        let current = read32(bar, REG_CSTS) & CSTS_RDY != 0;
        if current == ready {
            return true;
        }
        if read_timer().wrapping_sub(start) >= CLOCK_FREQ {
            return false;
        }
        spin_loop();
    }
}

fn read32(bar: usize, offset: usize) -> u32 {
    unsafe { read_volatile((bar + offset) as *const u32) }
}

fn write32(bar: usize, offset: usize, value: u32) {
    unsafe {
        write_volatile((bar + offset) as *mut u32, value);
    }
}

fn read64(bar: usize, offset: usize) -> u64 {
    let low = read32(bar, offset) as u64;
    let high = read32(bar, offset + 4) as u64;
    low | (high << 32)
}

fn write64(bar: usize, offset: usize, value: u64) {
    write32(bar, offset, value as u32);
    write32(bar, offset + 4, (value >> 32) as u32);
}
