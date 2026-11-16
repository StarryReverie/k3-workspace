//! 最小 PCIe 配置空间访问与设备枚举。
//!
//! 阶段 9A 只覆盖 QEMU `virt` 的 ECAM 主机桥：扫描 0 号总线，读取厂商、
//! 设备、类代码与 BAR，为后续 NVMe 驱动提供设备信息。不包含中断、BAR
//! 映射与桥设备递归。

use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};

use platform::println;

const PCI_VENDOR_ID: u16 = 0x00;
const PCI_COMMAND: u16 = 0x04;
const PCI_CLASS_REVISION: u16 = 0x08;
const PCI_HEADER_TYPE: u16 = 0x0c;
const PCI_BAR0: u16 = 0x10;

/// PCIe 设备地址。
#[derive(Clone, Copy, Debug)]
pub struct PciAddress {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
}

/// 设备 BAR 的描述。
#[derive(Clone, Copy, Debug)]
pub struct PciBar {
    pub index: u8,
    pub base: u64,
    pub size: u64,
    pub is_64: bool,
}

/// 枚举得到的 PCIe 设备。
#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
    pub address: PciAddress,
    pub vendor: u16,
    pub device: u16,
    /// 24 位类代码：prog_if << 16 | subclass << 8 | class。
    pub class: u32,
    pub header_type: u8,
    pub bars: [Option<PciBar>; 6],
}

/// PCIe 配置空间访问接口。
pub trait PciHost {
    fn read_config(&self, address: PciAddress, offset: u16) -> u32;
    fn write_config(&self, address: PciAddress, offset: u16, value: u32);
}

/// QEMU `virt` 的 ECAM 主机桥。
pub struct EcamHost {
    base: usize,
}

impl EcamHost {
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    fn config_addr(&self, address: PciAddress, offset: u16) -> usize {
        self.base
            + ((address.bus as usize) << 20)
            + ((address.dev as usize) << 15)
            + ((address.func as usize) << 12)
            + (offset as usize & 0xffc)
    }
}

impl PciHost for EcamHost {
    fn read_config(&self, address: PciAddress, offset: u16) -> u32 {
        unsafe { read_volatile(self.config_addr(address, offset) as *const u32) }
    }

    fn write_config(&self, address: PciAddress, offset: u16, value: u32) {
        unsafe {
            write_volatile(self.config_addr(address, offset) as *mut u32, value);
        }
    }
}

fn read_u8(host: &dyn PciHost, address: PciAddress, offset: u16) -> u8 {
    let shift = (offset & 3) * 8;
    (host.read_config(address, offset & !3) >> shift) as u8
}

/// 扫描 0 号总线并返回所有存在的设备。
pub fn enumerate(host: &dyn PciHost) -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for dev in 0..32u8 {
        let address = PciAddress {
            bus: 0,
            dev,
            func: 0,
        };
        let id = host.read_config(address, PCI_VENDOR_ID);
        if id == u32::MAX {
            continue;
        }
        let header_type = read_u8(host, address, PCI_HEADER_TYPE);
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

fn read_bar(host: &dyn PciHost, address: PciAddress, index: u8) -> Option<PciBar> {
    let offset = PCI_BAR0 + index as u16 * 4;
    let original = host.read_config(address, offset);
    if original == 0 {
        return None;
    }
    // I/O BAR 暂不支持，直接跳过。
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
        let upper_offset = offset + 4;
        if index >= 5 {
            return None;
        }
        let upper_original = host.read_config(address, upper_offset);
        host.write_config(address, upper_offset, u32::MAX);
        let upper_mask = host.read_config(address, upper_offset);
        host.write_config(address, upper_offset, upper_original);
        base |= (upper_original as u64) << 32;
        size_mask |= (upper_mask as u64) << 32;
    }

    // 32 位 BAR 的高 32 位不参与掩码，不能直接对 u64 取反。
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

/// 把 BAR 分配到指定的 CPU 地址，并写回配置空间。
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

/// 读取 BAR 当前基址。
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

/// 打开 Memory Space 与 Bus Master。
pub fn enable_memory_bus_master(host: &dyn PciHost, address: PciAddress) {
    let command = host.read_config(address, PCI_COMMAND);
    host.write_config(address, PCI_COMMAND, command | 0x6);
}

/// 打印设备列表与 NVMe 数量。
pub fn print_devices(devices: &[PciDevice]) {
    if devices.is_empty() {
        println!("PCIe: no device");
        return;
    }
    let nvme = devices
        .iter()
        .filter(|device| device.class == 0x010802)
        .count();
    println!("PCIe: {} device(s), {} NVMe", devices.len(), nvme);
    for device in devices {
        print_device(device);
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

/// 读取并打印 ECAM 主机桥下的设备。
///
/// 只用于阶段 9A 的只读探测；失败时打印原因，不影响系统启动。
pub fn probe_ecam(base: usize) {
    println!("PCIe: ECAM base={:#x}", base);
    let host = EcamHost::new(base);
    let devices = enumerate(&host);
    print_devices(&devices);
}
