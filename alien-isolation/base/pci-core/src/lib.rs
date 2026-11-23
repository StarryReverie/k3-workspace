//! PCI/PCIe enumeration and DesignWare iATU helpers.
//!
//! The crate is `no_std` in the kernel and builds as a normal library for
//! host-side unit tests. Configuration-space access is abstracted by
//! [`PciHost`]; DesignWare register access by [`DwIo`], so the K3 host bridge
//! can be exercised against a register model without hardware.

#![cfg_attr(not(test), no_std)]
extern crate alloc;

use alloc::vec::Vec;
use core::ops::Range;

pub const PCI_VENDOR_ID: u16 = 0x00;
pub const PCI_COMMAND: u16 = 0x04;
pub const PCI_CLASS_REVISION: u16 = 0x08;
pub const PCI_HEADER_TYPE: u16 = 0x0c;
pub const PCI_BAR0: u16 = 0x10;

pub const PCI_CLASS_NVME: u32 = 0x010802;

/// One PCIe function address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciAddress {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
}

/// A memory BAR as reported by the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciBar {
    pub index: u8,
    pub base: u64,
    pub size: u64,
    pub is_64: bool,
}

/// A discovered PCIe function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciDevice {
    pub address: PciAddress,
    pub vendor: u16,
    pub device: u16,
    /// `prog_if << 16 | subclass << 8 | class`.
    pub class: u32,
    pub header_type: u8,
    pub bars: [Option<PciBar>; 6],
}

/// Configuration-space access shared by ECAM and iATU hosts.
pub trait PciHost {
    fn read_config(&self, address: PciAddress, offset: u16) -> u32;
    fn write_config(&self, address: PciAddress, offset: u16, value: u32);
}

fn read_u8(host: &dyn PciHost, address: PciAddress, offset: u16) -> u8 {
    let shift = (offset & 3) * 8;
    (host.read_config(address, offset & !3) >> shift) as u8
}

/// Scan bus 0 and return every present function.
pub fn enumerate(host: &dyn PciHost) -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for dev in 0..32u8 {
        let probe = PciAddress {
            bus: 0,
            dev,
            func: 0,
        };
        if host.read_config(probe, PCI_VENDOR_ID) == u32::MAX {
            continue;
        }
        let header_type = read_u8(host, probe, PCI_HEADER_TYPE);
        let functions = if header_type & 0x80 != 0 { 8 } else { 1 };
        for func in 0..functions {
            let address = PciAddress { bus: 0, dev, func };
            let id = host.read_config(address, PCI_VENDOR_ID);
            if id == u32::MAX {
                continue;
            }
            devices.push(read_device(host, address, id, header_type));
        }
    }
    devices
}

fn read_device(host: &dyn PciHost, address: PciAddress, id: u32, header_type: u8) -> PciDevice {
    let class = host.read_config(address, PCI_CLASS_REVISION) >> 8;
    let mut bars = [None; 6];
    let mut index = 0u8;
    while index < 6 {
        if let Some(bar) = read_bar(host, address, index) {
            bars[index as usize] = Some(bar);
            index += if bar.is_64 { 2 } else { 1 };
        } else {
            index += 1;
        }
    }
    PciDevice {
        address,
        vendor: id as u16,
        device: (id >> 16) as u16,
        class,
        header_type,
        bars,
    }
}

/// Read one BAR and determine its size by writing an all-ones mask.
pub fn read_bar(host: &dyn PciHost, address: PciAddress, index: u8) -> Option<PciBar> {
    let offset = PCI_BAR0 + index as u16 * 4;
    let original = host.read_config(address, offset);
    if original == 0 {
        return None;
    }
    // I/O BARs are not used by NVMe; skip them.
    if original & 1 != 0 {
        return None;
    }

    let is_64 = (original >> 1) & 0x3 == 0x2;
    host.write_config(address, offset, u32::MAX);
    let mask = host.read_config(address, offset);
    host.write_config(address, offset, original);

    let mut base = (original & 0xffff_fff0) as u64;
    let mut size_mask = (mask & 0xffff_fff0) as u64;
    if is_64 {
        if index >= 5 {
            return None;
        }
        let upper_offset = offset + 4;
        let upper_original = host.read_config(address, upper_offset);
        host.write_config(address, upper_offset, u32::MAX);
        let upper_mask = host.read_config(address, upper_offset);
        host.write_config(address, upper_offset, upper_original);
        base |= (upper_original as u64) << 32;
        size_mask |= (upper_mask as u64) << 32;
    }

    // For a 32-bit BAR the upper half is not part of the mask.
    let size = if is_64 {
        (!size_mask).wrapping_add(1)
    } else {
        (!(mask & 0xffff_fff0)).wrapping_add(1) as u64
    };
    if size == 0 || size > (1u64 << 40) {
        return None;
    }
    Some(PciBar {
        index,
        base,
        size,
        is_64,
    })
}

/// Program a BAR base into configuration space.
pub fn assign_bar(host: &dyn PciHost, address: PciAddress, bar: &PciBar, base: u64) -> bool {
    if bar.size == 0 || base & (bar.size - 1) != 0 {
        return false;
    }
    let offset = PCI_BAR0 + bar.index as u16 * 4;
    host.write_config(address, offset, base as u32 & 0xffff_fff0);
    if bar.is_64 {
        host.write_config(address, offset + 4, (base >> 32) as u32);
    }
    true
}

/// Read the current BAR base from configuration space.
pub fn bar_base(host: &dyn PciHost, address: PciAddress, index: u8) -> u64 {
    let offset = PCI_BAR0 + index as u16 * 4;
    let lower = host.read_config(address, offset);
    let is_64 = (lower >> 1) & 0x3 == 0x2;
    let mut base = (lower & 0xffff_fff0) as u64;
    if is_64 {
        base |= (host.read_config(address, offset + 4) as u64) << 32;
    }
    base
}

/// Open Memory Space and Bus Master.
pub fn enable_memory_bus_master(host: &dyn PciHost, address: PciAddress) {
    let command = host.read_config(address, PCI_COMMAND);
    host.write_config(address, PCI_COMMAND, command | 0x6);
}

pub fn align_up(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    (value + align - 1) & !(align - 1)
}

// DesignWare iATU registers, relative to the selected region.
pub const IATU_VIEWPORT: usize = 0x900;
pub const IATU_VIEWPORT_BASE: usize = 0x904;
pub const ATU_REGION_CTRL1: usize = 0x00;
pub const ATU_REGION_CTRL2: usize = 0x04;
pub const ATU_LOWER_BASE: usize = 0x08;
pub const ATU_UPPER_BASE: usize = 0x0c;
pub const ATU_LIMIT: usize = 0x10;
pub const ATU_LOWER_TARGET: usize = 0x14;
pub const ATU_UPPER_TARGET: usize = 0x18;
pub const ATU_ENABLE: u32 = 1 << 31;
pub const ATU_DIR_IB: u32 = 1 << 31;
pub const ATU_TYPE_MEM: u32 = 0x0;
pub const ATU_TYPE_CFG0: u32 = 0x4;
/// Unrolled iATU: one 512-byte slot per region, inbound in the upper half.
pub const ATU_UNROLL_STRIDE: usize = 0x200;

/// Register access used by the DesignWare host bridge.
pub trait DwIo {
    fn read_dbi(&self, offset: usize) -> u32;
    fn write_dbi(&self, offset: usize, value: u32);
    fn read_link(&self, offset: usize) -> u32;
    fn read_config_window(&self, offset: usize) -> u32;
    fn write_config_window(&self, offset: usize, value: u32);
    fn config_base(&self) -> u64;
    fn read_atu(&self, offset: usize) -> u32;
    fn write_atu(&self, offset: usize, value: u32);
}

/// DesignWare host bridge with iATU-based configuration access.
pub struct SpacemitHost<T> {
    io: T,
    unrolled: bool,
}

impl<T: DwIo> SpacemitHost<T> {
    pub fn new(io: T) -> Self {
        // A readable viewport register means the older windowed layout; an
        // all-ones read means the region block is unrolled.
        let unrolled = io.read_dbi(IATU_VIEWPORT) == u32::MAX;
        Self { io, unrolled }
    }

    pub fn unrolled(&self) -> bool {
        self.unrolled
    }

    /// `PHY_AHB_LINK_STS`: bit1 SMLH_LINK_UP, bit12 RDLH_LINK_UP.
    pub fn link_status(&self) -> u32 {
        self.io.read_link(0x4)
    }

    pub fn link_up(&self) -> bool {
        let status = self.link_status();
        status & (1 << 1) != 0 && status & (1 << 12) != 0
    }

    fn atu_offset(&self, inbound: bool, index: u32, reg: usize) -> usize {
        let dir = if inbound { ATU_UNROLL_STRIDE } else { 0 };
        index as usize * ATU_UNROLL_STRIDE + dir + reg
    }

    fn write_region(&self, inbound: bool, index: u32, reg: usize, value: u32) {
        if self.unrolled {
            self.io.write_atu(self.atu_offset(inbound, index, reg), value);
        } else {
            let viewport = if inbound { ATU_DIR_IB } else { 0 } | index;
            self.io.write_dbi(IATU_VIEWPORT, viewport);
            self.io.write_dbi(IATU_VIEWPORT_BASE + reg, value);
        }
    }

    fn read_region(&self, inbound: bool, index: u32, reg: usize) -> u32 {
        if self.unrolled {
            self.io.read_atu(self.atu_offset(inbound, index, reg))
        } else {
            let viewport = if inbound { ATU_DIR_IB } else { 0 } | index;
            self.io.write_dbi(IATU_VIEWPORT, viewport);
            self.io.read_dbi(IATU_VIEWPORT_BASE + reg)
        }
    }

    fn program_outbound(
        &self,
        index: u32,
        cpu_base: u64,
        size: u64,
        pci_base: u64,
        kind: u32,
    ) -> bool {
        if size == 0 {
            return false;
        }
        self.write_region(false, index, ATU_REGION_CTRL1, kind);
        self.write_region(false, index, ATU_LOWER_BASE, cpu_base as u32);
        self.write_region(false, index, ATU_UPPER_BASE, (cpu_base >> 32) as u32);
        self.write_region(
            false,
            index,
            ATU_LIMIT,
            (cpu_base + size - 1) as u32,
        );
        self.write_region(false, index, ATU_LOWER_TARGET, pci_base as u32);
        self.write_region(false, index, ATU_UPPER_TARGET, (pci_base >> 32) as u32);
        self.write_region(false, index, ATU_REGION_CTRL2, ATU_ENABLE);
        for _ in 0..1000 {
            if self.read_region(false, index, ATU_REGION_CTRL2) & ATU_ENABLE != 0 {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    /// Program outbound region 0 for one configuration-space access.
    fn set_config_target(&self, address: PciAddress) -> bool {
        let target = ((address.bus as u64) << 20)
            | ((address.dev as u64) << 15)
            | ((address.func as u64) << 12);
        self.program_outbound(0, self.io.config_base(), 0x10000, target, ATU_TYPE_CFG0)
    }

    /// Map one BAR at `pci_addr` into the CPU `bar_window`.
    pub fn map_bar(&self, pci_addr: u64, bar_window: usize, size: u64) -> bool {
        self.program_outbound(1, bar_window as u64, size, pci_addr, ATU_TYPE_MEM)
    }
}

impl<T: DwIo> PciHost for SpacemitHost<T> {
    fn read_config(&self, address: PciAddress, offset: u16) -> u32 {
        if !self.set_config_target(address) {
            return u32::MAX;
        }
        self.io.read_config_window((offset as usize) & 0xffc)
    }

    fn write_config(&self, address: PciAddress, offset: u16, value: u32) {
        if self.set_config_target(address) {
            self.io
                .write_config_window((offset as usize) & 0xffc, value);
        }
    }
}

/// Helper for callers that keep a `Range` of a config window.
pub fn config_window(base: usize, size: usize) -> Range<usize> {
    base..base + size
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use core::cell::RefCell;

    const BAR_SIZE: u64 = 0x4000;

    /// Minimal flat config space for one NVMe function at 00:01.0.
    struct FakeEcam {
        words: RefCell<Vec<u32>>,
    }

    impl FakeEcam {
        fn new() -> Self {
            let mut words = vec![u32::MAX; 1024];
            // 00:00.0 host bridge
            words[0] = 0x0008_1b36;
            // 00:01.0 NVMe, 16 KiB prefetchable memory BAR0 (unassigned)
            words[4] = 0x8;
            Self {
                words: RefCell::new(words),
            }
        }
    }

    impl PciHost for FakeEcam {
        fn read_config(&self, address: PciAddress, offset: u16) -> u32 {
            if address.bus != 0 {
                return u32::MAX;
            }
            let index = offset as usize / 4;
            let words = self.words.borrow();
            match address.dev {
                0 => match index {
                    0 => words[0],
                    2 => 0x060000_00,
                    _ => 0,
                },
                1 => match index {
                    0 => 0x0010_1b36,
                    1 => words[1],
                    2 => 0x010802_00,
                    4 => current_bar(&words),
                    7 => 0,
                    _ => 0,
                },
                _ => u32::MAX,
            }
        }

        fn write_config(&self, address: PciAddress, offset: u16, value: u32) {
            if address.dev != 1 {
                return;
            }
            let index = offset as usize / 4;
            let mut words = self.words.borrow_mut();
            match index {
                4 => {
                    if value == u32::MAX {
                        words[16] = 1; // sizing flag
                    } else {
                        words[16] = 0;
                        words[4] = (value & 0xffff_fff0) | 0x8;
                    }
                }
                1 => words[1] = value,
                _ => {}
            }
        }
    }

    fn current_bar(words: &[u32]) -> u32 {
        if words[16] == 1 {
            !((BAR_SIZE - 1) as u32) & 0xffff_fff0
        } else {
            words[4] | 0x8
        }
    }

    #[test]
    fn enumerates_ecam_devices_and_sizes_bar() {
        let host = FakeEcam::new();
        let devices = enumerate(&host);
        assert_eq!(devices.len(), 2);
        let nvme = devices.iter().find(|d| d.class == PCI_CLASS_NVME).unwrap();
        assert_eq!(nvme.address.dev, 1);
        let bar = nvme.bars[0].unwrap();
        assert_eq!(bar.size, BAR_SIZE);
        assert!(!bar.is_64);
        assert_eq!(bar.base, 0);
        assert!(assign_bar(&host, nvme.address, &bar, 0x4000_0000));
        assert_eq!(bar_base(&host, nvme.address, 0), 0x4000_0000);
        enable_memory_bus_master(&host, nvme.address);
        let command = host.read_config(nvme.address, PCI_COMMAND);
        assert_eq!(command & 0x6, 0x6);
    }

    /// DesignWare register model with an unrolled iATU.
    struct FakeDw {
        dbi: RefCell<Vec<u32>>,
        atu: RefCell<Vec<u32>>,
        viewport_regs: RefCell<Vec<u32>>,
        link: RefCell<Vec<u32>>,
        config: RefCell<Vec<u32>>,
        config_base: u64,
        unrolled: bool,
        target: RefCell<u64>,
        outbound: RefCell<Vec<Option<(u64, u64, u64, u32)>>>,
    }

    impl FakeDw {
        fn new(unrolled: bool, link_up: bool) -> Self {
            let mut link = vec![0u32; 4];
            if link_up {
                link[1] = (1 << 1) | (1 << 12);
            }
            let dbi = vec![0u32; 0x1000 / 4];
            let dbi = if unrolled {
                let mut dbi = dbi;
                dbi[IATU_VIEWPORT / 4] = u32::MAX;
                dbi
            } else {
                dbi
            };
            Self {
                dbi: RefCell::new(dbi),
                atu: RefCell::new(vec![0u32; 0x4000 / 4]),
                viewport_regs: RefCell::new(vec![0u32; 8]),
                link: RefCell::new(link),
                config: RefCell::new(vec![0u32; 0x10000 / 4]),
                config_base: 0x11_0000_0000,
                unrolled,
                target: RefCell::new(0),
                outbound: RefCell::new(vec![None; 8]),
            }
        }
    }

    impl DwIo for FakeDw {
        fn read_dbi(&self, offset: usize) -> u32 {
            if !self.unrolled && offset >= IATU_VIEWPORT_BASE {
                let reg = (offset - IATU_VIEWPORT_BASE) / 4;
                return self.viewport_regs.borrow().get(reg).copied().unwrap_or(0);
            }
            self.dbi.borrow()[offset / 4]
        }

        fn write_dbi(&self, offset: usize, value: u32) {
            if !self.unrolled && offset >= IATU_VIEWPORT_BASE {
                let reg = (offset - IATU_VIEWPORT_BASE) / 4;
                if let Some(slot) = self.viewport_regs.borrow_mut().get_mut(reg) {
                    *slot = value;
                }
                if offset - IATU_VIEWPORT_BASE == ATU_LOWER_TARGET {
                    *self.target.borrow_mut() = value as u64;
                }
                return;
            }
            self.dbi.borrow_mut()[offset / 4] = value;
        }

        fn read_link(&self, offset: usize) -> u32 {
            self.link.borrow()[offset / 4]
        }

        fn read_config_window(&self, offset: usize) -> u32 {
            // Only dword 0 (vendor/device) and dword 2 (class) are used.
            match offset / 4 {
                0 => 0x0010_1b36,
                2 => 0x010802_00,
                4 => !((BAR_SIZE - 1) as u32) & 0xffff_fff0,
                _ => 0,
            }
        }

        fn write_config_window(&self, offset: usize, value: u32) {
            let mut config = self.config.borrow_mut();
            let index = offset / 4;
            if index < config.len() {
                config[index] = value;
            }
        }

        fn config_base(&self) -> u64 {
            self.config_base
        }

        fn read_atu(&self, offset: usize) -> u32 {
            self.atu.borrow()[offset / 4]
        }

        fn write_atu(&self, offset: usize, value: u32) {
            let mut atu = self.atu.borrow_mut();
            let index = offset / 4;
            if index < atu.len() {
                atu[index] = value;
            }
            if offset < ATU_UNROLL_STRIDE && offset == ATU_LOWER_TARGET {
                *self.target.borrow_mut() = value as u64;
            }
            // Region 1: 0x200 + reg; remember the programming for assertions.
            if offset >= ATU_UNROLL_STRIDE && offset < ATU_UNROLL_STRIDE * 2 {
                let reg = offset - ATU_UNROLL_STRIDE;
                let mut outbound = self.outbound.borrow_mut();
                let slot = outbound[1].get_or_insert((0, 0, 0, 0));
                match reg {
                    ATU_LOWER_BASE => slot.0 = value as u64,
                    ATU_LIMIT => slot.1 = (value as u64) + 1 - slot.0,
                    ATU_LOWER_TARGET => slot.2 = value as u64,
                    ATU_REGION_CTRL2 => slot.3 = value,
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn detects_unrolled_and_viewport() {
        let unrolled = SpacemitHost::new(FakeDw::new(true, true));
        assert!(unrolled.unrolled());
        let viewport = SpacemitHost::new(FakeDw::new(false, true));
        assert!(!viewport.unrolled());
    }

    #[test]
    fn config_access_programs_outbound_zero() {
        let host = SpacemitHost::new(FakeDw::new(true, true));
        let address = PciAddress {
            bus: 0,
            dev: 1,
            func: 0,
        };
        assert_eq!(host.read_config(address, PCI_VENDOR_ID), 0x0010_1b36);
        // Region 0 target must be the config-space dev/func address.
        let target = *host.io.target.borrow();
        assert_eq!(target, (1 << 15));
    }

    #[test]
    fn viewport_config_access_programs_target() {
        let host = SpacemitHost::new(FakeDw::new(false, true));
        assert!(!host.unrolled());
        let address = PciAddress {
            bus: 0,
            dev: 2,
            func: 0,
        };
        assert_eq!(host.read_config(address, PCI_VENDOR_ID), 0x0010_1b36);
        assert_eq!(*host.io.target.borrow(), 2 << 15);
    }

    #[test]
    fn link_state_uses_bits_one_and_twelve() {
        let up = SpacemitHost::new(FakeDw::new(true, true));
        assert!(up.link_up());
        assert_eq!(up.link_status(), (1 << 1) | (1 << 12));
        let down = SpacemitHost::new(FakeDw::new(true, false));
        assert!(!down.link_up());
    }

    #[test]
    fn map_bar_programs_outbound_one() {
        let host = SpacemitHost::new(FakeDw::new(true, false));
        assert!(host.map_bar(0x4000_0000, 0x4010_0000, BAR_SIZE));
        let outbound = host.io.outbound.borrow();
        let region = outbound[1].unwrap();
        assert_eq!(region.0, 0x4010_0000);
        assert_eq!(region.1, BAR_SIZE);
        assert_eq!(region.2, 0x4000_0000);
        assert_eq!(region.3 & ATU_ENABLE, ATU_ENABLE);
    }
}
