//! Polling NVMe driver for the `nvme` isolation domain.
//!
//! The controller is driven through a `SafeIORegion` covering BAR0. Queues and
//! data pages come from `FrameTracker`, which returns identity-mapped physical
//! frames, so queue addresses can be used as PRP values directly.

use core::hint::spin_loop;

use basic::{
    io::SafeIORegion,
    println,
    time::read_timer,
    vm::frame::FrameTracker,
};

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

/// NVMe driver result type.
pub type NvmeResult<T> = Result<T, &'static str>;

/// One DMA page backed by an identity-mapped frame.
struct DmaPage {
    frame: FrameTracker,
}

impl DmaPage {
    fn new() -> Self {
        let frame = FrameTracker::new(1);
        frame.clear();
        Self { frame }
    }

    fn addr(&self) -> u64 {
        self.frame.start_phy_addr().as_usize() as u64
    }

    fn bytes(&self) -> &[u8] {
        self.frame.as_slice_with(0)
    }

    /// Device-visible view of this page, used for submission queue entries.
    fn io(&self) -> SafeIORegion {
        let start = self.frame.start_phy_addr().as_usize();
        SafeIORegion::from(start..start + self.frame.size())
    }
}

/// 64-byte NVMe submission queue entry.
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
        self.dwords[6] = address as u32;
        self.dwords[7] = (address >> 32) as u32;
    }

    fn set_prp2(&mut self, address: u64) {
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

    /// Write the entry with 8-byte volatile stores.
    ///
    /// The controller may fetch the entry as soon as the doorbell is rung, so
    /// the opcode and CID (both in dword 0) must become visible together.
    fn write_volatile_to(&self, page: &DmaPage, slot: usize) {
        let io = page.io();
        for index in 0..8 {
            let value = (self.dwords[index * 2] as u64)
                | ((self.dwords[index * 2 + 1] as u64) << 32);
            let _ = io.write_at(slot + index * 8, value);
        }
    }
}

/// Identify result needed by the block layer.
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

/// NVMe controller with one admin and one I/O queue pair.
pub struct NvmeController {
    io: SafeIORegion,
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
    blocks: u64,
    last_io_slot: usize,
    last_io_cid: u16,
}

impl NvmeController {
    pub fn new(io: SafeIORegion) -> NvmeResult<Self> {
        let cap = read64(&io, REG_CAP);
        let doorbell_stride = 4usize << ((cap >> 32) & 0xf) as usize;

        write32(&io, REG_INTMS, u32::MAX);
        write32(&io, REG_CC, 0);
        if !wait_ready(&io, false) {
            println!("NVMe: CSTS={:#x} while stopping", read32(&io, REG_CSTS));
            return Err("controller did not stop");
        }

        let admin_sq = DmaPage::new();
        let admin_cq = DmaPage::new();
        write64(&io, REG_ASQ, admin_sq.addr());
        write64(&io, REG_ACQ, admin_cq.addr());
        write32(
            &io,
            REG_AQA,
            (((ADMIN_QUEUE_SIZE - 1) as u32) << 16) | (ADMIN_QUEUE_SIZE - 1) as u32,
        );
        write32(&io, REG_CC, CC_IOSQES | CC_IOCQES | CC_EN);
        if !wait_ready(&io, true) {
            println!("NVMe: CSTS={:#x} while starting", read32(&io, REG_CSTS));
            return Err("controller did not start");
        }
        println!("NVMe: controller ready, cap={:#x}", cap);

        let mut controller = Self {
            io,
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
            blocks: 0,
            last_io_slot: 0,
            last_io_cid: 0,
        };
        controller.setup_io_queues()?;
        Ok(controller)
    }

    pub fn identify(&mut self) -> NvmeResult<NvmeIdentify> {
        let page = DmaPage::new();

        let mut command = NvmeCommand::new(0x06);
        command.set_prp1(page.addr());
        command.set_cdw10(0x01);
        let _ = self.submit_admin(&mut command)?;

        let data = page.bytes();
        let mut model = [0u8; 40];
        let mut serial = [0u8; 20];
        let mut firmware = [0u8; 8];
        model.copy_from_slice(&data[24..64]);
        serial.copy_from_slice(&data[4..24]);
        firmware.copy_from_slice(&data[64..72]);

        page.frame.clear();
        let mut command = NvmeCommand::new(0x06);
        command.set_nsid(1);
        command.set_prp1(page.addr());
        command.set_cdw10(0x00);
        let _ = self.submit_admin(&mut command)?;

        let data = page.bytes();
        let nsze = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let flbas = (data[26] & 0x0f) as usize;
        let offset = 128 + flbas * 4;
        let lbaf = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        let lba_size = 1u32 << ((lbaf >> 16) & 0xf);

        self.nsid = 1;
        self.block_size = lba_size;
        self.blocks = nsze;
        Ok(NvmeIdentify {
            model,
            serial,
            firmware,
            nsze,
            lba_size,
        })
    }

    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    pub fn blocks(&self) -> u64 {
        self.blocks
    }

    /// Read `buf.len() / block_size` logical blocks starting at `start_lba`.
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

        // A read is idempotent. If the controller reports a completion with an
        // unexpected CID (seen intermittently under QEMU TCG), resubmit once
        // or twice instead of failing the filesystem read.
        for attempt in 0..3 {
            match self.submit_io(&mut command) {
                Ok(()) => return Ok(()),
                Err(error)
                    if error == "I/O completion cid mismatch" && attempt < 2 =>
                {
                    println!(
                        "NVMe: resubmitting read after completion mismatch (attempt {})",
                        attempt + 1
                    );
                }
                Err(error) => return Err(error),
            }
        }
        Err("I/O completion cid mismatch")
    }

    fn setup_io_queues(&mut self) -> NvmeResult<()> {
        let mut command = NvmeCommand::new(0x09);
        command.set_cdw10(0x07);
        command.set_cdw11(0);
        let result = self.submit_admin(&mut command)?;
        println!("NVMe: queue result={:#x}", result);

        let mut command = NvmeCommand::new(0x05);
        command.set_prp1(self.io_cq.addr());
        command.set_cdw10((((IO_QUEUE_SIZE - 1) as u32) << 16) | IO_QID as u32);
        command.set_cdw11(1);
        let _ = self.submit_admin(&mut command)?;

        let mut command = NvmeCommand::new(0x01);
        command.set_prp1(self.io_sq.addr());
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
        command.write_volatile_to(&self.admin_sq, slot);
        self.admin_sq_tail = (self.admin_sq_tail + 1) % ADMIN_QUEUE_SIZE;
        arch::fence_io();
        write32(&self.io, self.sq_doorbell(0), self.admin_sq_tail as u32);

        self.wait_admin(cid)
    }

    fn wait_admin(&mut self, cid: u16) -> NvmeResult<u32> {
        let start = read_timer();
        loop {
            let entry = self.admin_cq_head as usize * 16;
            let cq = self.admin_cq.bytes();
            let dw3 = read_u32(cq, entry + 12);
            let phase = (dw3 >> 16) & 1 != 0;
            if phase == self.admin_phase {
                let received = (dw3 & 0xffff) as u16;
                let status = (dw3 >> 16) & 0xfffe;
                let result = read_u32(cq, entry);
                self.admin_cq_head = (self.admin_cq_head + 1) % ADMIN_QUEUE_SIZE;
                if self.admin_cq_head == 0 {
                    self.admin_phase = !self.admin_phase;
                }
                arch::fence_io();
                write32(&self.io, self.cq_doorbell(0), self.admin_cq_head as u32);
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
            if read_timer().wrapping_sub(start) >= basic::config::CLOCK_FREQ {
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
        command.write_volatile_to(&self.io_sq, slot);
        self.last_io_slot = slot;
        self.last_io_cid = cid;
        self.io_sq_tail = (self.io_sq_tail + 1) % IO_QUEUE_SIZE;
        arch::fence_io();
        write32(&self.io, self.sq_doorbell(IO_QID), self.io_sq_tail as u32);

        self.wait_io(cid)
    }

    fn wait_io(&mut self, cid: u16) -> NvmeResult<()> {
        let start = read_timer();
        loop {
            let entry = self.io_cq_head as usize * 16;
            let cq = self.io_cq.bytes();
            let dw3 = read_u32(cq, entry + 12);
            let phase = (dw3 >> 16) & 1 != 0;
            if phase == self.io_phase {
                let received = (dw3 & 0xffff) as u16;
                let status = (dw3 >> 16) & 0xfffe;
                if received != cid {
                    let sq = self.io_sq.bytes();
                    let slot = self.last_io_slot;
                    println!(
                        "NVMe: I/O completion cid {} != {} (head={} phase={} dw0={:#x} dw1={:#x} dw2={:#x} dw3={:#x})",
                        received,
                        cid,
                        self.io_cq_head,
                        self.io_phase,
                        read_u32(cq, entry),
                        read_u32(cq, entry + 4),
                        read_u32(cq, entry + 8),
                        dw3
                    );
                    println!(
                        "NVMe: last SQE slot={} cid={} dwords={:#x},{:#x},{:#x},{:#x}",
                        slot,
                        self.last_io_cid,
                        read_u32(sq, slot),
                        read_u32(sq, slot + 16),
                        read_u32(sq, slot + 24),
                        read_u32(sq, slot + 28)
                    );
                    for index in 0..IO_QUEUE_SIZE as usize {
                        let dw0 = read_u32(sq, index * 64);
                        if dw0 != 0 {
                            println!("NVMe: SQE[{}] dw0={:#x}", index, dw0);
                        }
                    }
                }
                self.io_cq_head = (self.io_cq_head + 1) % IO_QUEUE_SIZE;
                if self.io_cq_head == 0 {
                    self.io_phase = !self.io_phase;
                }
                arch::fence_io();
                write32(&self.io, self.cq_doorbell(IO_QID), self.io_cq_head as u32);
                if received != cid {
                    return Err("I/O completion cid mismatch");
                }
                if status != 0 {
                    println!("NVMe: I/O status={:#x}", status);
                    return Err("I/O command failed");
                }
                return Ok(());
            }
            if read_timer().wrapping_sub(start) >= basic::config::CLOCK_FREQ {
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

/// Build PRP1/PRP2 for `buf`.
///
/// Up to two pages are described directly; longer buffers use one list page
/// holding the remaining page addresses.
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
    let entries: &mut [u64] = list.frame.as_mut_slice_with(0);
    for index in 1..pages {
        entries[index - 1] = (first + index * 0x1000) as u64;
    }
    (start as u64, list.addr())
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn wait_ready(io: &SafeIORegion, ready: bool) -> bool {
    let start = read_timer();
    loop {
        let current = read32(io, REG_CSTS) & CSTS_RDY != 0;
        if current == ready {
            return true;
        }
        if read_timer().wrapping_sub(start) >= basic::config::CLOCK_FREQ {
            return false;
        }
        spin_loop();
    }
}

fn read32(io: &SafeIORegion, offset: usize) -> u32 {
    io.read_at(offset).unwrap_or(u32::MAX)
}

fn write32(io: &SafeIORegion, offset: usize, value: u32) {
    let _ = io.write_at(offset, value);
}

fn read64(io: &SafeIORegion, offset: usize) -> u64 {
    let low = read32(io, offset) as u64;
    let high = read32(io, offset + 4) as u64;
    low | (high << 32)
}

fn write64(io: &SafeIORegion, offset: usize, value: u64) {
    write32(io, offset, value as u32);
    write32(io, offset + 4, (value >> 32) as u32);
}
