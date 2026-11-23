//! Minimal PCIe configuration-space access and device enumeration.
//!
//! Two host bridges are supported:
//!
//! - QEMU `virt`: generic ECAM (`pci-host-ecam-generic`).
//! - SpacemiT K3: DesignWare host bridge with a separate iATU block.
//!
//! The TCB only enumerates devices, assigns the NVMe BAR and hands the MMIO
//! range to the `nvme` block device domain. NVMe protocol logic stays in the
//! domain.

use alloc::{string::String, vec::Vec};
use core::ops::Range;

use basic::io::SafeIORegion;
use ksync::Mutex;
use platform::println;

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

/// Configuration-space access shared by both host bridges.
pub trait PciHost {
    fn read_config(&self, address: PciAddress, offset: u16) -> u32;
    fn write_config(&self, address: PciAddress, offset: u16, value: u32);
}

/// PCIe ECAM host bridge (QEMU `virt`).
pub struct EcamHost {
    io: SafeIORegion,
}

impl EcamHost {
    pub fn new(range: Range<usize>) -> Self {
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

// DesignWare iATU registers, relative to the selected region.
const IATU_VIEWPORT: usize = 0x900;
const IATU_VIEWPORT_BASE: usize = 0x904;
const ATU_REGION_CTRL1: usize = 0x00;
const ATU_REGION_CTRL2: usize = 0x04;
const ATU_LOWER_BASE: usize = 0x08;
const ATU_UPPER_BASE: usize = 0x0c;
const ATU_LIMIT: usize = 0x10;
const ATU_LOWER_TARGET: usize = 0x14;
const ATU_UPPER_TARGET: usize = 0x18;
const ATU_ENABLE: u32 = 1 << 31;
const ATU_DIR_IB: u32 = 1 << 31;
const ATU_TYPE_MEM: u32 = 0x0;
const ATU_TYPE_CFG0: u32 = 0x4;
/// Unrolled iATU: one 512-byte slot per region, inbound in the upper half.
const ATU_UNROLL_STRIDE: usize = 0x200;

/// SpacemiT K3 DesignWare host bridge.
pub struct SpacemitHost {
    dbi: SafeIORegion,
    atu: SafeIORegion,
    link: SafeIORegion,
    config: SafeIORegion,
    unrolled: bool,
}

impl SpacemitHost {
    pub fn new(info: &SpacemitPciInfo) -> Self {
        let dbi = SafeIORegion::from(info.dbi.clone());
        let atu = SafeIORegion::from(info.atu.clone());
        let link = SafeIORegion::from(info.link.clone());
        let config = SafeIORegion::from(info.config.clone());
        // A readable viewport register means the older windowed layout; an
        // all-ones read means the region block is unrolled.
        let unrolled = dbi.read_at::<u32>(IATU_VIEWPORT).map(|v| v == u32::MAX).unwrap_or(false);
        Self {
            dbi,
            atu,
            link,
            config,
            unrolled,
        }
    }

    pub fn unrolled(&self) -> bool {
        self.unrolled
    }

    /// `PHY_AHB_LINK_STS`: bit1 SMLH_LINK_UP, bit12 RDLH_LINK_UP.
    pub fn link_status(&self) -> u32 {
        self.link.read_at::<u32>(0x4).unwrap_or(0)
    }

    pub fn link_up(&self) -> bool {
        let status = self.link_status();
        status & (1 << 1) != 0 && status & (1 << 12) != 0
    }

    fn atu_offset(&self, inbound: bool, index: u32, reg: usize) -> usize {
        let dir = if inbound { ATU_UNROLL_STRIDE } else { 0 };
        index as usize * ATU_UNROLL_STRIDE + dir + reg
    }

    fn select_region(&self, inbound: bool, index: u32) -> usize {
        if self.unrolled {
            self.atu_offset(inbound, index, 0)
        } else {
            let viewport = if inbound { ATU_DIR_IB } else { 0 } | index;
            let _ = self.dbi.write_at(IATU_VIEWPORT, viewport);
            IATU_VIEWPORT_BASE
        }
    }

    fn write_atu(&self, inbound: bool, index: u32, reg: usize, value: u32) {
        let base = self.select_region(inbound, index);
        let _ = self.dbi_or_atu(base + reg, value);
    }

    fn read_atu(&self, inbound: bool, index: u32, reg: usize) -> u32 {
        let base = self.select_region(inbound, index);
        self.dbi_or_atu_read(base + reg)
    }

    /// Unrolled mode lives in the separate `atu` region; viewport mode keeps
    /// the region registers right after the viewport in DBI space.
    fn dbi_or_atu(&self, offset: usize, value: u32) {
        let _ = if self.unrolled {
            self.atu.write_at(offset, value)
        } else {
            self.dbi.write_at(offset, value)
        };
    }

    fn dbi_or_atu_read(&self, offset: usize) -> u32 {
        if self.unrolled {
            self.atu.read_at(offset).unwrap_or(u32::MAX)
        } else {
            self.dbi.read_at(offset).unwrap_or(u32::MAX)
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
        self.write_atu(false, index, ATU_REGION_CTRL1, kind);
        self.write_atu(false, index, ATU_LOWER_BASE, cpu_base as u32);
        self.write_atu(false, index, ATU_UPPER_BASE, (cpu_base >> 32) as u32);
        self.write_atu(
            false,
            index,
            ATU_LIMIT,
            (cpu_base + size - 1) as u32,
        );
        self.write_atu(false, index, ATU_LOWER_TARGET, pci_base as u32);
        self.write_atu(false, index, ATU_UPPER_TARGET, (pci_base >> 32) as u32);
        self.write_atu(false, index, ATU_REGION_CTRL2, ATU_ENABLE);
        for _ in 0..1000 {
            if self.read_atu(false, index, ATU_REGION_CTRL2) & ATU_ENABLE != 0 {
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
        let cpu = self.config.phys_addr().as_usize() as u64;
        self.program_outbound(0, cpu, 0x10000, target, ATU_TYPE_CFG0)
    }

    /// Map one BAR at `pci_addr` into the CPU `bar_window`.
    pub fn map_bar(&self, pci_addr: u64, bar_window: usize, size: u64) -> bool {
        self.program_outbound(
            1,
            bar_window as u64,
            size,
            pci_addr,
            ATU_TYPE_MEM,
        )
    }
}

impl PciHost for SpacemitHost {
    fn read_config(&self, address: PciAddress, offset: u16) -> u32 {
        if !self.set_config_target(address) {
            return u32::MAX;
        }
        self.config
            .read_at((offset as usize) & 0xffc)
            .unwrap_or(u32::MAX)
    }

    fn write_config(&self, address: PciAddress, offset: u16, value: u32) {
        if self.set_config_target(address) {
            let _ = self
                .config
                .write_at((offset as usize) & 0xffc, value);
        }
    }
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

fn read_u8(host: &dyn PciHost, address: PciAddress, offset: u16) -> u8 {
    let shift = (offset & 3) * 8;
    (host.read_config(address, offset & !3) >> shift) as u8
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

fn read_bar(host: &dyn PciHost, address: PciAddress, index: u8) -> Option<PciBar> {
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

fn assign_bar(host: &dyn PciHost, address: PciAddress, bar: &PciBar, base: u64) -> bool {
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

fn bar_base(host: &dyn PciHost, address: PciAddress, index: u8) -> u64 {
    let offset = PCI_BAR0 + index as u16 * 4;
    let lower = host.read_config(address, offset);
    let is_64 = (lower >> 1) & 0x3 == 0x2;
    let mut base = (lower & 0xffff_fff0) as u64;
    if is_64 {
        base |= (host.read_config(address, offset + 4) as u64) << 32;
    }
    base
}

fn enable_memory_bus_master(host: &dyn PciHost, address: PciAddress) {
    let command = host.read_config(address, PCI_COMMAND);
    host.write_config(address, PCI_COMMAND, command | 0x6);
}

fn align_up(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    (value + align - 1) & !(align - 1)
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

    let host = SpacemitHost::new(&info);
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

    let devices = enumerate(&host);
    if devices.is_empty() {
        println!("K3 PCIe: link up but no device found");
        return;
    }
    let nvme_count = devices
        .iter()
        .filter(|device| device.class == PCI_CLASS_NVME)
        .count();
    println!("K3 PCIe: {} device(s), {} NVMe", devices.len(), nvme_count);
    for device in &devices {
        print_device(device);
    }

    let window_start = platform::config::K3_PCI_BAR_WINDOW_START as u64;
    let window_end = platform::config::K3_PCI_BAR_WINDOW_END as u64;
    for device in &devices {
        if device.class != PCI_CLASS_NVME {
            continue;
        }
        let Some(bar) = device.bars.iter().flatten().find(|bar| bar.size > 0) else {
            println!("K3 PCIe: NVMe has no memory BAR, skip");
            continue;
        };
        let assigned = align_up(window_start, bar.size);
        if assigned + bar.size > window_end {
            println!(
                "K3 PCIe: BAR window too small for {:#x} bytes",
                bar.size
            );
            continue;
        }
        if !assign_bar(&host, device.address, bar, assigned) {
            println!("K3 PCIe: BAR assignment failed");
            continue;
        }
        enable_memory_bus_master(&host, device.address);
        let start = bar_base(&host, device.address, bar.index);
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
