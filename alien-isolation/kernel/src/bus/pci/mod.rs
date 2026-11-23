//! PCIe device discovery for the TCB.
//!
//! Generic enumeration and DesignWare iATU programming live in `pci-core`
//! (unit-tested against a register model). This module supplies the real MMIO
//! access, platform policy and the NVMe BAR hand-off to the `nvme` domain.

use alloc::string::String;
use core::ops::Range;

use basic::io::SafeIORegion;
use ksync::Mutex;
use pci_core::{PciAddress, PciHost};
use platform::println;

mod device;
use device::PciBus;

/// Scaffolding for the PCI bus driver registry, kept for later stages.
static PCI_BUS: Mutex<PciBus> = Mutex::new(PciBus::new());

/// Host bridge description produced by the device-tree probe.
#[derive(Clone, Debug)]
pub struct PciHostInfo {
    pub compatible: String,
    /// Generic ECAM window (QEMU).
    pub ecam: Option<Range<usize>>,
    /// DesignWare host bridge (K3).
    pub spacemit: Option<SpacemitPciInfo>,
}

/// MMIO ranges of one `spacemit,k3-pcie` controller.
#[derive(Clone, Debug)]
pub struct SpacemitPciInfo {
    pub port: u32,
    pub lanes: u32,
    pub dbi: Range<usize>,
    pub dbi2: Range<usize>,
    pub atu: Range<usize>,
    pub config: Range<usize>,
    pub app: Range<usize>,
    pub link: Range<usize>,
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

/// PCIe ECAM host bridge (QEMU `virt`).
struct EcamHost {
    io: SafeIORegion,
}

impl EcamHost {
    fn new(range: Range<usize>) -> Self {
        Self {
            io: SafeIORegion::from(range),
        }
    }

    fn config_offset(address: PciAddress, offset: u16) -> usize {
        ((address.bus as usize) << 20)
            + ((address.dev as usize) << 15)
            + ((address.func as usize) << 12)
            + (offset as usize & 0xffc)
    }
}

impl PciHost for EcamHost {
    fn read_config(&self, address: PciAddress, offset: u16) -> u32 {
        self.io
            .read_at(Self::config_offset(address, offset))
            .unwrap_or(u32::MAX)
    }

    fn write_config(&self, address: PciAddress, offset: u16, value: u32) {
        let _ = self
            .io
            .write_at(Self::config_offset(address, offset), value);
    }
}

/// Real MMIO access for one SpacemiT K3 DesignWare controller.
struct RealDw {
    dbi: SafeIORegion,
    atu: SafeIORegion,
    link: SafeIORegion,
    config: SafeIORegion,
}

impl RealDw {
    fn new(info: &SpacemitPciInfo) -> Self {
        Self {
            dbi: SafeIORegion::from(info.dbi.clone()),
            atu: SafeIORegion::from(info.atu.clone()),
            link: SafeIORegion::from(info.link.clone()),
            config: SafeIORegion::from(info.config.clone()),
        }
    }
}

impl pci_core::DwIo for RealDw {
    fn read_dbi(&self, offset: usize) -> u32 {
        self.dbi.read_at(offset).unwrap_or(u32::MAX)
    }

    fn write_dbi(&self, offset: usize, value: u32) {
        let _ = self.dbi.write_at(offset, value);
    }

    fn read_link(&self, offset: usize) -> u32 {
        self.link.read_at(offset).unwrap_or(0)
    }

    fn read_config_window(&self, offset: usize) -> u32 {
        self.config.read_at(offset).unwrap_or(u32::MAX)
    }

    fn write_config_window(&self, offset: usize, value: u32) {
        let _ = self.config.write_at(offset, value);
    }

    fn config_base(&self) -> u64 {
        self.config.phys_addr().as_usize() as u64
    }

    fn read_atu(&self, offset: usize) -> u32 {
        self.atu.read_at(offset).unwrap_or(u32::MAX)
    }

    fn write_atu(&self, offset: usize, value: u32) {
        let _ = self.atu.write_at(offset, value);
    }
}

/// Dispatch to the host bridge described by the device tree.
pub fn pci_init(info: PciHostInfo) {
    if let Some(ecam) = info.ecam.clone() {
        init_ecam(ecam);
        return;
    }
    if let Some(spacemit) = info.spacemit.clone() {
        init_spacemit(spacemit);
        return;
    }
    println!("PCIe: host bridge has no usable address range, skip");
}

fn init_ecam(ecam: Range<usize>) {
    let base = ecam.start;
    let size = ecam.end - base;
    println!("PCIe: ECAM base={:#x} size={:#x}", base, size);

    let host = EcamHost::new(ecam);
    let devices = pci_core::enumerate(&host);
    if devices.is_empty() {
        println!("PCIe: no device");
        return;
    }
    let nvme_count = devices
        .iter()
        .filter(|device| device.class == pci_core::PCI_CLASS_NVME)
        .count();
    println!("PCIe: {} device(s), {} NVMe", devices.len(), nvme_count);
    for device in &devices {
        print_device(device);
    }

    let window_start = platform::config::PCI_BAR_WINDOW_START as u64;
    let window_end = platform::config::PCI_BAR_WINDOW_END as u64;
    let mut cursor = window_start;
    for device in &devices {
        if device.class != pci_core::PCI_CLASS_NVME {
            continue;
        }
        let Some(bar) = device.bars.iter().flatten().find(|bar| bar.size > 0) else {
            println!("PCIe: NVMe has no memory BAR, skip");
            continue;
        };
        let assigned = pci_core::align_up(cursor, bar.size);
        if assigned + bar.size > window_end {
            println!(
                "PCIe: BAR window exhausted (need {:#x} at {:#x})",
                bar.size, assigned
            );
            continue;
        }
        if !pci_core::assign_bar(&host, device.address, bar, assigned) {
            println!("PCIe: BAR assignment failed");
            continue;
        }
        cursor = assigned + bar.size;
        pci_core::enable_memory_bus_master(&host, device.address);
        let start = pci_core::bar_base(&host, device.address, bar.index) as usize;
        println!(
            "PCIe: NVMe BAR{} -> {:#x} size={:#x}, memory+bus master on",
            bar.index, start, bar.size
        );
        *NVME_DEVICE.lock() = Some(NvmeBar {
            bar: start..start + bar.size as usize,
        });
    }
}

fn init_spacemit(info: SpacemitPciInfo) {
    println!(
        "K3 PCIe: port={} lanes={} dbi={:#x}-{:#x} atu={:#x}-{:#x} config={:#x}-{:#x}",
        info.port,
        info.lanes,
        info.dbi.start,
        info.dbi.end,
        info.atu.start,
        info.atu.end,
        info.config.start,
        info.config.end
    );

    let host = pci_core::SpacemitHost::new(RealDw::new(&info));
    println!(
        "K3 PCIe: iATU {}",
        if host.unrolled() {
            "unrolled"
        } else {
            "viewport"
        }
    );

    let status = host.link_status();
    if !host.link_up() {
        println!(
            "K3 PCIe: port={} link down, PHY_AHB_LINK_STS={:#x}, skip NVMe",
            info.port, status
        );
        return;
    }
    println!(
        "K3 PCIe: port={} link up, PHY_AHB_LINK_STS={:#x}",
        info.port, status
    );

    let devices = pci_core::enumerate(&host);
    if devices.is_empty() {
        println!("K3 PCIe: link up but no device found");
        return;
    }
    let nvme_count = devices
        .iter()
        .filter(|device| device.class == pci_core::PCI_CLASS_NVME)
        .count();
    println!("K3 PCIe: {} device(s), {} NVMe", devices.len(), nvme_count);
    for device in &devices {
        print_device(device);
    }

    let window_start = platform::config::K3_PCI_BAR_WINDOW_START as u64;
    let window_end = platform::config::K3_PCI_BAR_WINDOW_END as u64;
    for device in &devices {
        if device.class != pci_core::PCI_CLASS_NVME {
            continue;
        }
        let Some(bar) = device.bars.iter().flatten().find(|bar| bar.size > 0) else {
            println!("K3 PCIe: NVMe has no memory BAR, skip");
            continue;
        };
        let assigned = pci_core::align_up(window_start, bar.size);
        if assigned + bar.size > window_end {
            println!("K3 PCIe: BAR window too small for {:#x} bytes", bar.size);
            continue;
        }
        if !pci_core::assign_bar(&host, device.address, bar, assigned) {
            println!("K3 PCIe: BAR assignment failed");
            continue;
        }
        pci_core::enable_memory_bus_master(&host, device.address);
        let start = pci_core::bar_base(&host, device.address, bar.index);
        if !host.map_bar(start, window_start as usize, bar.size) {
            println!("K3 PCIe: iATU outbound mapping failed");
            continue;
        }
        println!(
            "K3 PCIe: NVMe BAR{} -> PCI {:#x}, CPU {:#x} size={:#x}",
            bar.index, start, window_start, bar.size
        );
        *NVME_DEVICE.lock() = Some(NvmeBar {
            bar: window_start as usize..window_start as usize + bar.size as usize,
        });
    }
}

fn print_device(device: &pci_core::PciDevice) {
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
