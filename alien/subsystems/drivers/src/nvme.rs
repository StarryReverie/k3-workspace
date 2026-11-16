//! 最小 NVMe 轮询驱动。
//!
//! 阶段 9A 只覆盖控制器复位、管理队列、命令提交与 Identify。I/O 队列与
//! 块设备接口在后续循环加入。所有队列与数据页使用内核恒等映射，地址可
//! 直接作为物理地址使用。

use core::{
    hint::spin_loop,
    ptr::{read_volatile, write_volatile},
    sync::atomic::{fence, Ordering},
};

use mem::{alloc_frames, free_frames};
use platform::{config::CLOCK_FREQ, println};
use timer::read_timer;

const ADMIN_QUEUE_SIZE: u16 = 32;
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

    fn set_cdw10(&mut self, value: u32) {
        self.dwords[10] = value;
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
    next_cid: u16,
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

        Ok(Self {
            bar,
            doorbell_stride,
            admin_sq,
            admin_cq,
            admin_sq_tail: 0,
            admin_cq_head: 0,
            admin_phase: true,
            next_cid: 1,
        })
    }

    pub fn identify(&mut self) -> NvmeResult<NvmeIdentify> {
        let page = DmaPage::new();

        let mut command = NvmeCommand::new(0x06);
        command.set_prp1(page.addr() as u64);
        command.set_cdw10(0x01);
        self.submit_admin(&mut command)?;

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
        self.submit_admin(&mut command)?;

        let data =
            unsafe { core::slice::from_raw_parts(page.addr() as *const u8, config::FRAME_SIZE) };
        let nsze = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let flbas = (data[26] & 0x0f) as usize;
        let offset = 128 + flbas * 4;
        let lbaf = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        let lba_size = 1u32 << ((lbaf >> 16) & 0xf);

        Ok(NvmeIdentify {
            model,
            serial,
            firmware,
            nsze,
            lba_size,
        })
    }

    fn submit_admin(&mut self, command: &mut NvmeCommand) -> NvmeResult<()> {
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
        write32(self.bar, REG_DOORBELL_BASE, self.admin_sq_tail as u32);

        self.wait_admin(cid)
    }

    fn wait_admin(&mut self, cid: u16) -> NvmeResult<()> {
        let start = read_timer();
        loop {
            let entry = (self.admin_cq.addr() + self.admin_cq_head as usize * 16) as *const u32;
            let dw3 = unsafe { read_volatile(entry.add(3)) };
            let phase = (dw3 >> 16) & 1 != 0;
            if phase == self.admin_phase {
                let received = (dw3 & 0xffff) as u16;
                let status = (dw3 >> 16) & 0xfffe;
                self.admin_cq_head = (self.admin_cq_head + 1) % ADMIN_QUEUE_SIZE;
                if self.admin_cq_head == 0 {
                    self.admin_phase = !self.admin_phase;
                }
                fence(Ordering::SeqCst);
                write32(
                    self.bar,
                    REG_DOORBELL_BASE + self.doorbell_stride,
                    self.admin_cq_head as u32,
                );
                if received != cid {
                    println!("NVMe: completion cid {} != {}", received, cid);
                    return Err("completion cid mismatch");
                }
                if status != 0 {
                    println!("NVMe: admin status={:#x}", status);
                    return Err("admin command failed");
                }
                return Ok(());
            }
            if read_timer().wrapping_sub(start) >= CLOCK_FREQ {
                return Err("admin command timeout");
            }
            spin_loop();
        }
    }
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
