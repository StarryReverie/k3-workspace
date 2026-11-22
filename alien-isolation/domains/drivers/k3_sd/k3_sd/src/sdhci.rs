//! Minimal polling SDHCI driver for the SpacemiT K3 SD controller.
//!
//! The controller follows the SDHCI register layout, but its Present State
//! buffer-ready bits are two positions higher than the generic specification.
//! Only PIO single-block reads are implemented; M4 mounts the card read-only.

use basic::{config::CLOCK_FREQ, io::SafeIORegion, println, time::read_timer};

const REG_ARGUMENT: usize = 0x008;
const REG_BLOCK_COUNT: usize = 0x006;
const REG_BLOCK_SIZE: usize = 0x004;
const REG_BUFFER: usize = 0x020;
const REG_CAPABILITIES: usize = 0x040;
const REG_CLOCK_CONTROL: usize = 0x02c;
const REG_COMMAND: usize = 0x00e;
const REG_ERROR_INT_SIGNAL_EN: usize = 0x03a;
const REG_ERROR_INT_STATUS: usize = 0x032;
const REG_ERROR_INT_STATUS_EN: usize = 0x036;
const REG_HOST_CONTROL: usize = 0x028;
const REG_NORMAL_INT_SIGNAL_EN: usize = 0x038;
const REG_NORMAL_INT_STATUS: usize = 0x030;
const REG_NORMAL_INT_STATUS_EN: usize = 0x034;
const REG_POWER_CONTROL: usize = 0x029;
const REG_PRESENT_STATE: usize = 0x024;
const REG_RESPONSE: usize = 0x010;
const REG_SOFTWARE_RESET: usize = 0x02f;
const REG_TIMEOUT_CONTROL: usize = 0x02e;
const REG_TRANSFER_MODE: usize = 0x00c;

const PS_CMD_INHIBIT: u32 = 1 << 0;
const PS_DAT_INHIBIT: u32 = 1 << 1;
/// K3 shifts the generic buffer-ready bits by two.
const PS_BUFFER_READ_ENABLE: u32 = 1 << 11;

const INT_CMD_COMPLETE: u16 = 1 << 0;
const INT_TRANSFER_COMPLETE: u16 = 1 << 1;
const INT_ERROR: u16 = 1 << 15;

const CLOCK_INTERNAL_ENABLE: u16 = 1 << 0;
const CLOCK_INTERNAL_STABLE: u16 = 1 << 1;
const CLOCK_SD_ENABLE: u16 = 1 << 2;

const POWER_3V3: u8 = 0x0f;
const HOST_CTRL_4BIT: u8 = 0x02;

const CMD_RESP_NONE: u16 = 0;
const CMD_RESP_136: u16 = 0x01;
const CMD_RESP_48: u16 = 0x02;
const CMD_RESP_48_BUSY: u16 = 0x03;
const CMD_CRC: u16 = 0x08;
const CMD_INDEX: u16 = 0x10;
const CMD_DATA: u16 = 0x20;

const TM_READ: u16 = 0x10;
const TM_BLOCK_COUNT_ENABLE: u16 = 0x02;

pub const BLOCK_SIZE: usize = 512;

struct SdhciHost {
    io: SafeIORegion,
    base_clock: u32,
    rca: u16,
    high_capacity: bool,
}

impl SdhciHost {
    fn new(io: SafeIORegion) -> Self {
        Self {
            io,
            base_clock: 200_000_000,
            rca: 0,
            high_capacity: false,
        }
    }

    fn read8(&self, offset: usize) -> u8 {
        self.io.read_at::<u8>(offset).unwrap()
    }

    fn read16(&self, offset: usize) -> u16 {
        self.io.read_at::<u16>(offset).unwrap()
    }

    fn read32(&self, offset: usize) -> u32 {
        self.io.read_at::<u32>(offset).unwrap()
    }

    fn write8(&self, offset: usize, value: u8) {
        self.io.write_at::<u8>(offset, value).unwrap()
    }

    fn write16(&self, offset: usize, value: u16) {
        self.io.write_at::<u16>(offset, value).unwrap()
    }

    fn write32(&self, offset: usize, value: u32) {
        self.io.write_at::<u32>(offset, value).unwrap()
    }

    fn delay_ms(&self, ms: usize) {
        let start = read_timer();
        let ticks = ms * CLOCK_FREQ / 1000;
        while read_timer().wrapping_sub(start) < ticks {
            core::hint::spin_loop();
        }
    }

    fn wait_state(&self, bits: u32, value: bool, timeout_ms: usize) -> bool {
        let start = read_timer();
        let ticks = timeout_ms * CLOCK_FREQ / 1000;
        loop {
            let set = self.read32(REG_PRESENT_STATE) & bits != 0;
            if set == value {
                return true;
            }
            if read_timer().wrapping_sub(start) >= ticks {
                return false;
            }
            core::hint::spin_loop();
        }
    }

    fn wait_status(&self, bits: u16, timeout_ms: usize) -> bool {
        let start = read_timer();
        let ticks = timeout_ms * CLOCK_FREQ / 1000;
        loop {
            if self.read16(REG_NORMAL_INT_STATUS) & bits == bits {
                return true;
            }
            if read_timer().wrapping_sub(start) >= ticks {
                return false;
            }
            core::hint::spin_loop();
        }
    }

    fn clear_status(&self) {
        self.write16(REG_NORMAL_INT_STATUS, 0xffff);
        self.write16(REG_ERROR_INT_STATUS, 0xffff);
    }

    fn reset(&self) -> bool {
        self.write8(REG_SOFTWARE_RESET, 0x01);
        let start = read_timer();
        let ticks = CLOCK_FREQ / 10;
        while self.read8(REG_SOFTWARE_RESET) & 0x01 != 0 {
            if read_timer().wrapping_sub(start) >= ticks {
                return false;
            }
            core::hint::spin_loop();
        }
        self.write8(REG_TIMEOUT_CONTROL, 0x0e);
        true
    }

    fn detect_base_clock(&mut self) {
        let caps = self.read32(REG_CAPABILITIES);
        let mhz = ((caps >> 8) & 0xff) as u32;
        if mhz > 0 {
            self.base_clock = mhz * 1_000_000;
        }
    }

    fn set_clock(&self, target: u32) -> bool {
        let mut control = self.read16(REG_CLOCK_CONTROL);
        control &= !CLOCK_SD_ENABLE;
        self.write16(REG_CLOCK_CONTROL, control);

        let divisor = (self.base_clock / (2 * target.max(1))).clamp(1, 0xff) as u16;
        let control = (divisor << 8) | CLOCK_INTERNAL_ENABLE;
        self.write16(REG_CLOCK_CONTROL, control);

        let start = read_timer();
        let ticks = CLOCK_FREQ / 10;
        loop {
            if self.read16(REG_CLOCK_CONTROL) & CLOCK_INTERNAL_STABLE != 0 {
                break;
            }
            if read_timer().wrapping_sub(start) >= ticks {
                return false;
            }
            core::hint::spin_loop();
        }
        self.write16(REG_CLOCK_CONTROL, control | CLOCK_SD_ENABLE);
        true
    }

    fn command(&self, index: u32, arg: u32, flags: u16) -> Result<u32, ()> {
        if !self.wait_state(PS_CMD_INHIBIT, false, 200) {
            return Err(());
        }
        if flags & CMD_DATA != 0 && !self.wait_state(PS_DAT_INHIBIT, false, 200) {
            return Err(());
        }
        self.clear_status();
        self.write32(REG_ARGUMENT, arg);
        self.write16(REG_COMMAND, ((index as u16) << 8) | flags);
        if !self.wait_status(INT_CMD_COMPLETE, 200) {
            return Err(());
        }
        if flags & 0x03 == CMD_RESP_48_BUSY && !self.wait_state(PS_DAT_INHIBIT, false, 500) {
            return Err(());
        }
        let error = self.read16(REG_ERROR_INT_STATUS);
        if error != 0 {
            return Err(());
        }
        let normal = self.read16(REG_NORMAL_INT_STATUS);
        self.write16(REG_NORMAL_INT_STATUS, normal);
        if normal & INT_ERROR != 0 {
            return Err(());
        }
        Ok(self.read32(REG_RESPONSE))
    }

    fn card_init(&mut self) -> bool {
        if self.command(0, 0, CMD_RESP_NONE).is_err() {
            println!("SDHCI: CMD0 failed");
            return false;
        }

        let if_cond = self.command(8, 0x1aa, CMD_RESP_48 | CMD_CRC | CMD_INDEX);
        let sdhc = match if_cond {
            Ok(value) => value & 0xfff == 0x1aa,
            Err(_) => {
                println!(
                    "SDHCI: CMD8 failed (error={:#x})",
                    self.read16(REG_ERROR_INT_STATUS)
                );
                false
            }
        };

        let mut ready = false;
        let mut last = 0u32;
        for round in 0..50 {
            let _ = self.command(55, 0, CMD_RESP_48 | CMD_CRC | CMD_INDEX);
            match self.command(41, 0x40ff_8000, CMD_RESP_48) {
                Ok(value) => {
                    last = value;
                    if value & 0x8000_0000 != 0 {
                        ready = true;
                        self.high_capacity = value & (1 << 30) != 0;
                        break;
                    }
                }
                Err(_) => {
                    if round < 3 {
                        println!(
                            "SDHCI: ACMD41 error={:#x}",
                            self.read16(REG_ERROR_INT_STATUS)
                        );
                    }
                }
            }
            self.delay_ms(10);
            if round >= 2 && last == 0 {
                break;
            }
        }
        if !ready {
            println!("SDHCI: ACMD41 timeout, last={:#x}", last);
            return false;
        }

        if self.command(2, 0, CMD_RESP_136 | CMD_CRC).is_err() {
            println!("SDHCI: CMD2 failed");
            return false;
        }
        let relative = match self.command(3, 0, CMD_RESP_48 | CMD_CRC | CMD_INDEX) {
            Ok(value) => value,
            Err(_) => {
                println!("SDHCI: CMD3 failed");
                return false;
            }
        };
        self.rca = ((relative >> 16) & 0xffff) as u16;

        let arg = (self.rca as u32) << 16;
        if self
            .command(7, arg, CMD_RESP_48_BUSY | CMD_CRC | CMD_INDEX)
            .is_err()
        {
            println!("SDHCI: CMD7 failed");
            return false;
        }

        let _ = self.command(55, arg, CMD_RESP_48 | CMD_CRC | CMD_INDEX);
        if self
            .command(6, 2, CMD_RESP_48 | CMD_CRC | CMD_INDEX)
            .is_err()
        {
            println!("SDHCI: ACMD6 failed");
            return false;
        }
        self.write8(REG_HOST_CONTROL, self.read8(REG_HOST_CONTROL) | HOST_CTRL_4BIT);

        let _ = self.command(16, BLOCK_SIZE as u32, CMD_RESP_48 | CMD_CRC | CMD_INDEX);
        println!(
            "SDHCI: card ready rca={:#x} v2={} high_capacity={}",
            self.rca, sdhc, self.high_capacity
        );
        true
    }

    fn init(&mut self) -> bool {
        self.detect_base_clock();
        println!(
            "SDHCI: base={:#x} base_clock={}MHz state={:#x} clock={:#x} power={:#x} caps={:#x}",
            self.io.phys_addr().as_usize(),
            self.base_clock / 1_000_000,
            self.read32(REG_PRESENT_STATE),
            self.read16(REG_CLOCK_CONTROL),
            self.read8(REG_POWER_CONTROL),
            self.read32(REG_CAPABILITIES)
        );
        if !self.reset() {
            println!("SDHCI: reset timeout");
            return false;
        }
        self.write8(REG_POWER_CONTROL, POWER_3V3);
        self.delay_ms(2);
        if !self.set_clock(400_000) {
            println!("SDHCI: 400kHz clock timeout");
            return false;
        }
        self.write16(REG_NORMAL_INT_STATUS_EN, 0xffff);
        self.write16(REG_ERROR_INT_STATUS_EN, 0xffff);
        self.write16(REG_NORMAL_INT_SIGNAL_EN, 0);
        self.write16(REG_ERROR_INT_SIGNAL_EN, 0);
        self.clear_status();
        if !self.card_init() {
            return false;
        }
        if !self.set_clock(25_000_000) {
            println!("SDHCI: 25MHz clock timeout");
        }
        true
    }

    fn block_argument(&self, block: u64) -> u32 {
        if self.high_capacity {
            block as u32
        } else {
            (block * BLOCK_SIZE as u64) as u32
        }
    }

    fn read_block(&mut self, block: u64, buf: &mut [u8]) -> Result<(), ()> {
        if buf.len() != BLOCK_SIZE {
            return Err(());
        }
        self.write16(REG_BLOCK_SIZE, BLOCK_SIZE as u16);
        self.write16(REG_BLOCK_COUNT, 1);
        self.write16(REG_TRANSFER_MODE, TM_READ | TM_BLOCK_COUNT_ENABLE);
        self.command(
            17,
            self.block_argument(block),
            CMD_RESP_48 | CMD_CRC | CMD_INDEX | CMD_DATA,
        )?;

        let mut offset = 0;
        while offset < BLOCK_SIZE {
            if !self.wait_state(PS_BUFFER_READ_ENABLE, true, 500) {
                return Err(());
            }
            let word = self.read32(REG_BUFFER);
            buf[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
            offset += 4;
        }
        if !self.wait_status(INT_TRANSFER_COMPLETE, 500) {
            return Err(());
        }
        self.write16(REG_NORMAL_INT_STATUS, INT_TRANSFER_COMPLETE);
        let error = self.read16(REG_ERROR_INT_STATUS);
        if error != 0 {
            self.write16(REG_ERROR_INT_STATUS, error);
            return Err(());
        }
        Ok(())
    }
}

/// Block device exposing the first FAT partition of the SD card.
pub struct SdhciBlockDevice {
    host: SdhciHost,
    part_lba: u64,
    part_blocks: usize,
}

impl SdhciBlockDevice {
    pub fn new(io: SafeIORegion) -> Self {
        Self {
            host: SdhciHost::new(io),
            part_lba: 0,
            part_blocks: 0,
        }
    }

    pub fn init(&mut self) -> bool {
        if !self.host.init() {
            return false;
        }

        let mut mbr = [0u8; BLOCK_SIZE];
        if self.host.read_block(0, &mut mbr).is_err() {
            println!("SDHCI: read MBR failed");
            return false;
        }
        let Some((lba, blocks)) = parse_mbr(&mbr) else {
            println!("SDHCI: no FAT partition in MBR");
            return false;
        };

        let mut boot = [0u8; BLOCK_SIZE];
        if self.host.read_block(lba, &mut boot).is_err() {
            println!("SDHCI: read boot sector failed");
            return false;
        }
        if boot[510] != 0x55 || boot[511] != 0xaa {
            println!(
                "SDHCI: partition {} is not formatted (signature {:02x}{:02x})",
                lba, boot[510], boot[511]
            );
            return false;
        }

        self.part_lba = lba;
        self.part_blocks = blocks;
        println!(
            "SDHCI: partition lba={} blocks={} ({} MiB)",
            lba,
            blocks,
            blocks * BLOCK_SIZE / 1024 / 1024
        );
        true
    }

    pub fn read_block(&mut self, block: u32, buf: &mut [u8]) -> Result<(), ()> {
        self.host
            .read_block(self.part_lba + block as u64, buf)
    }

    pub fn capacity(&self) -> u64 {
        self.part_blocks as u64
    }
}

/// Parse the MBR and return the first FAT partition's start and sector count.
fn parse_mbr(mbr: &[u8; BLOCK_SIZE]) -> Option<(u64, usize)> {
    if mbr[510] != 0x55 || mbr[511] != 0xaa {
        return None;
    }
    let mut fallback = None;
    for index in 0..4 {
        let entry = 0x1be + index * 16;
        let part_type = mbr[entry + 4];
        if part_type == 0 {
            continue;
        }
        let lba = u32::from_le_bytes(mbr[entry + 8..entry + 12].try_into().ok()?) as u64;
        let sectors = u32::from_le_bytes(mbr[entry + 12..entry + 16].try_into().ok()?) as usize;
        if sectors == 0 {
            continue;
        }
        if matches!(part_type, 0x01 | 0x04 | 0x06 | 0x0b | 0x0c) {
            return Some((lba, sectors));
        }
        if fallback.is_none() {
            fallback = Some((lba, sectors));
        }
    }
    fallback
}
