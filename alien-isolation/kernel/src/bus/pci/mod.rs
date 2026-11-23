//! Minimal PCIe ECAM configuration-space access and device enumeration.
//!
//! Phase M5A only covers the QEMU `virt` host bridge (`pci-host-ecam-generic`):
//! scan bus 0, size BARs, assign the NVMe BAR from a fixed 32-bit window and
//! hand the resulting MMIO range to the `nvme` block device domain.
//!
//! K3 uses a DesignWare host bridge that needs iATU programming; that path is
//! added in M5B and is not handled here.

use alloc::vec::Vec;
use core::ops::Range;

use basic::io::SafeIORegion;
use ksync::Mutex;
use platform::println;

use crate::bus::CommonDeviceInfo;

mod device;
use device::PciBus;

/// Scaffolding for the PCI bus driver registry, kept for later stages.
static PCI_BUS: Mutex<PciBus> = Mutex::new(PciBus::new());

const PCI_VENDOR_ID: u16 = 0x00;
const PCI_COMMAND: u16 = 0x04;
const PCI_CLASS_REVISION: u16 = 0x08;
const PCI_HEADER_TYPE: u16 = 0x0c;
const PCI_BAR0: u16 = 0x10;

const PCI_CLASS_NVME: u32 = 0x010802;

/// One PCIe function address.
#[derive(Clone, Copy, Debug)]
pub struct PciAddress {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
}

/// A memory BAR as reported by the device.
#[derive(Clone, Copy, Debug)]
pub struct PciBar {
    pub index: u8,
    pub base: u64,
    pub size: u64,
    pub is_64: bool,
}

/// A discovered PCIe function.
#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
    pub address: PciAddress,
    pub vendor: u16,
    pub device: u16,
    /// `prog_if << 16 | subclass << 8 | class`.
    pub class: u32,
    pub header_type: u8,
    pub bars: [Option<PciBar>; 6],
}

/// MMIO range reserved for the NVMe domain.
#[derive(Clone, Debug)]
pub struct NvmeBar {
    pub bar: Range<usize>,
}

static NVME_DEVICE: Mutex<Option<NvmeBar>> = Mutex::new(None);

/// Take the NVMe BAR discovered during PCIe enumeration, if any.
pub fn take_nvme() -> Option<NvmeBar> {
    NVME_DEVICE.lock().take()
}

/// PCIe ECAM host bridge.
pub struct EcamHost {
    io: SafeIORegion,
}

impl EcamHost {
    pub fn new(range: Range<mem::PhysAddr>) -> Self {
        Self {
            io: SafeIORegion::new(range),
        }
    }

    fn config_offset(address: PciAddress, offset: u16) -> usize {
        ((address.bus as usize) << 20)
            + ((address.dev as usize) << 15)
            + ((address.func as usize) << 12)
            + (offset as usize & 0xffc)
    }

    pub fn read_config(&self, address: PciAddress, offset: u16) -> u32 {
        self.io
            .read_at(Self::config_offset(address, offset))
            .unwrap_or(u32::MAX)
    }

    pub fn write_config(&self, address: PciAddress, offset: u16, value: u32) {
        let _ = self.io.write_at(Self::config_offset(address, offset), value);
    }
}

/// Scan bus 0 and return every present function.
pub fn enumerate(host: &EcamHost) -> Vec<PciDevice> {
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

fn read_u8(host: &EcamHost, address: PciAddress, offset: u16) -> u8 {
    let shift = (offset & 3) * 8;
    (host.read_config(address, offset & !3) >> shift) as u8
}

fn read_device(host: &EcamHost, address: PciAddress, id: u32, header_type: u8) -> PciDevice {
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

fn read_bar(host: &EcamHost, address: PciAddress, index: u8) -> Option<PciBar> {
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

fn assign_bar(host: &EcamHost, address: PciAddress, bar: &PciBar, base: u64) -> bool {
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

fn bar_base(host: &EcamHost, address: PciAddress, index: u8) -> u64 {
    let offset = PCI_BAR0 + index as u16 * 4;
    let lower = host.read_config(address, offset);
    let is_64 = (lower >> 1) & 0x3 == 0x2;
    let mut base = (lower & 0xffff_fff0) as u64;
    if is_64 {
        base |= (host.read_config(address, offset + 4) as u64) << 32;
    }
    base
}

fn enable_memory_bus_master(host: &EcamHost, address: PciAddress) {
    let command = host.read_config(address, PCI_COMMAND);
    host.write_config(address, PCI_COMMAND, command | 0x6);
}

fn align_up(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    (value + align - 1) & !(align - 1)
}

/// Enumerate the ECAM bus, assign an NVMe BAR and remember its MMIO range.
pub fn pci_init(pci_info: CommonDeviceInfo) {
    let base = pci_info.address_range.start.as_usize();
    let size = pci_info.address_range.end.as_usize() - base;
    println!("PCIe: ECAM base={:#x} size={:#x}", base, size);

    let host = EcamHost::new(pci_info.address_range.clone());
    let devices = enumerate(&host);
    if devices.is_empty() {
        println!("PCIe: no device");
        return;
    }
    let nvme_count = devices
        .iter()
        .filter(|device| device.class == PCI_CLASS_NVME)
        .count();
    println!("PCIe: {} device(s), {} NVMe", devices.len(), nvme_count);
    for device in &devices {
        print_device(device);
    }

    let window_start = platform::config::PCI_BAR_WINDOW_START as u64;
    let window_end = platform::config::PCI_BAR_WINDOW_END as u64;
    let mut cursor = window_start;
    for device in &devices {
        if device.class != PCI_CLASS_NVME {
            continue;
        }
        let Some(bar) = device.bars.iter().flatten().find(|bar| bar.size > 0) else {
            println!("PCIe: NVMe has no memory BAR, skip");
            continue;
        };
        let assigned = align_up(cursor, bar.size);
        if assigned + bar.size > window_end {
            println!(
                "PCIe: BAR window exhausted (need {:#x} at {:#x})",
                bar.size, assigned
            );
            continue;
        }
        if !assign_bar(&host, device.address, bar, assigned) {
            println!("PCIe: BAR assignment failed");
            continue;
        }
        cursor = assigned + bar.size;
        enable_memory_bus_master(&host, device.address);
        let start = bar_base(&host, device.address, bar.index) as usize;
        println!(
            "PCIe: NVMe BAR{} -> {:#x} size={:#x}, memory+bus master on",
            bar.index, start, bar.size
        );
        *NVME_DEVICE.lock() = Some(NvmeBar {
            bar: start..start + bar.size as usize,
        });
    }
}

fn print_device(device: &PciDevice) {
    let address = device.address;
    println!(
        "PCIe: {:02x}:{:02x}.{} vendor={:#06x} device={:#06x} class={:#08x} header={:#04x}",
        address.bus,
        address.dev,
        address.func,
        device.vendor,
        device.device,
        device.class,
        device.header_type
    );
    for bar in device.bars.iter().flatten() {
        println!(
            "PCIe:   bar{} base={:#x} size={:#x} {}",
            bar.index,
            bar.base,
            bar.size,
            if bar.is_64 { "mem64" } else { "mem32" }
        );
    }
}
