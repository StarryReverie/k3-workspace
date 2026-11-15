use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::ops::Range;

use fdt::{node::FdtNode, standard_nodes::Compatible, Fdt};

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub base_addr: usize,
    pub irq: usize,
    pub compatible: String,
}

impl DeviceInfo {
    pub fn new(name: String, base_addr: usize, irq: usize, compatible: String) -> Self {
        Self {
            name,
            base_addr,
            irq,
            compatible,
        }
    }
}

/// S-level IMSIC 的探测结果。
#[derive(Debug, Clone, Copy)]
pub struct ImsicInfo {
    pub base: usize,
    pub size: usize,
    pub num_ids: usize,
    pub hart_index_bits: u8,
    pub guest_index_bits: u8,
}

/// S-level APLIC 的探测结果。
#[derive(Debug, Clone, Copy)]
pub struct AplicInfo {
    pub base: usize,
    pub size: usize,
    pub num_sources: usize,
}

fn compatible_contains(node: FdtNode<'_, '_>, needle: &str) -> bool {
    node.compatible()
        .map(|compatible| compatible.all().any(|name| name.contains(needle)))
        .unwrap_or(false)
}

fn node_is_reserved(node: FdtNode<'_, '_>) -> bool {
    node.property("status")
        .map(|property| property.value.starts_with(b"reserved"))
        .unwrap_or(false)
}

fn node_reg(node: FdtNode<'_, '_>) -> Option<(usize, usize)> {
    let region = node.reg()?.next()?;
    Some((
        region.starting_address as usize,
        region.size.unwrap_or(0),
    ))
}

fn node_u32(node: FdtNode<'_, '_>, name: &str) -> Option<usize> {
    let property = node.property(name)?;
    match property.value.len() {
        4 => Some(u32::from_be_bytes(property.value.try_into().ok()?) as usize),
        8 => Some(u64::from_be_bytes(property.value.try_into().ok()?) as usize),
        _ => None,
    }
}

fn node_irq(node: FdtNode<'_, '_>) -> Option<usize> {
    if let Some(property) = node.property("interrupts") {
        if property.value.len() >= 4 {
            return Some(u32::from_be_bytes(property.value[..4].try_into().ok()?) as usize);
        }
    }
    let property = node.property("interrupts-extended")?;
    let mut chunks = property.value.chunks_exact(8);
    let chunk = chunks.next()?;
    Some(u32::from_be_bytes(chunk[4..8].try_into().ok()?) as usize)
}

fn imsic_is_supervisor(node: FdtNode<'_, '_>) -> bool {
    let Some(property) = node.property("interrupts-extended") else {
        return false;
    };
    property
        .value
        .chunks_exact(8)
        .any(|chunk| u32::from_be_bytes(chunk[4..8].try_into().unwrap()) == 9)
}

pub trait Probe {
    /// Get the base address and irq number of the uart device from the device tree.
    fn probe_uart(&self) -> Option<DeviceInfo>;
    /// Get the base address and irq number of the rtc device from the device tree.
    fn probe_rtc(&self) -> Option<DeviceInfo>;
    /// Get the base address and irq number of the virtio devices from the device tree.
    fn probe_virtio(&self) -> Option<Vec<DeviceInfo>>;
    /// Get the usable memory range from the device tree.
    fn probe_memory(&self) -> Option<Range<usize>>;
    /// Get the hart IDs from the device tree.
    fn probe_harts(&self) -> Vec<usize>;
    /// Get the supervisor-level IMSIC information from the device tree.
    fn probe_imsic(&self) -> Option<ImsicInfo>;
    /// Get the supervisor-level APLIC information from the device tree.
    fn probe_aplic(&self) -> Option<AplicInfo>;
    fn probe_common(&self, device_name: &str) -> Option<DeviceInfo>;
    #[cfg(feature = "vf2")]
    fn probe_sdio(&self) -> Option<DeviceInfo> {
        self.probe_common("sdio1")
    }
}

impl Probe for Fdt<'_> {
    fn probe_uart(&self) -> Option<DeviceInfo> {
        match self.probe_common("uart") {
            Some(device_info) => Some(device_info),
            None => self.probe_common("serial"),
        }
    }

    fn probe_rtc(&self) -> Option<DeviceInfo> {
        self.probe_common("rtc")
    }

    fn probe_virtio(&self) -> Option<Vec<DeviceInfo>> {
        let mut virtio_devices = Vec::new();
        for node in self.all_nodes() {
            if node.name.starts_with("virtio_mmio") {
                let reg = node.reg()?.next()?;
                let paddr = reg.starting_address as usize;
                let irq = node.property("interrupts")?.value;
                let irq = u32::from_be_bytes(irq[0..4].try_into().ok()?);

                let compatible = node.compatible().map(Compatible::first).unwrap();

                virtio_devices.push(DeviceInfo::new(
                    String::from("virtio_mmio"),
                    paddr,
                    irq as usize,
                    compatible.to_string(),
                ));
            }
        }
        Some(virtio_devices)
    }

    fn probe_memory(&self) -> Option<Range<usize>> {
        for node in self.all_nodes() {
            if node.name != "memory" && !node.name.starts_with("memory@") {
                continue;
            }
            if let Some((start, size)) = node_reg(node) {
                if size > 0 {
                    return Some(start..start + size);
                }
            }
        }
        None
    }

    fn probe_harts(&self) -> Vec<usize> {
        self.cpus()
            .flat_map(|cpu| cpu.ids().all())
            .collect::<Vec<_>>()
    }

    fn probe_imsic(&self) -> Option<ImsicInfo> {
        let node = self
            .all_nodes()
            .filter(|node| compatible_contains(*node, "imsics"))
            .filter(|node| !node_is_reserved(*node))
            .find(|node| imsic_is_supervisor(*node))?;
        let (base, size) = node_reg(node)?;
        Some(ImsicInfo {
            base,
            size,
            num_ids: node_u32(node, "riscv,num-ids").unwrap_or(0),
            hart_index_bits: node_u32(node, "riscv,hart-index-bits").unwrap_or(0) as u8,
            guest_index_bits: node_u32(node, "riscv,guest-index-bits").unwrap_or(0) as u8,
        })
    }

    fn probe_aplic(&self) -> Option<AplicInfo> {
        let imsic_phandle = self
            .all_nodes()
            .filter(|node| compatible_contains(*node, "imsics"))
            .filter(|node| !node_is_reserved(*node))
            .filter(|node| imsic_is_supervisor(*node))
            .find_map(|node| node_u32(node, "phandle").map(|value| value as u32))?;

        for node in self.all_nodes() {
            if !compatible_contains(node, "aplic") || node_is_reserved(node) {
                continue;
            }
            let Some(parent) = node.property("msi-parent") else {
                continue;
            };
            if parent.value.len() < 4 {
                continue;
            }
            let phandle = u32::from_be_bytes(parent.value[..4].try_into().ok()?);
            if phandle != imsic_phandle {
                continue;
            }
            let (base, size) = node_reg(node)?;
            return Some(AplicInfo {
                base,
                size,
                num_sources: node_u32(node, "riscv,num-sources").unwrap_or(0),
            });
        }
        None
    }

    fn probe_common(&self, device_name: &str) -> Option<DeviceInfo> {
        for node in self.all_nodes() {
            if !node.name.starts_with(device_name) {
                continue;
            }
            let Some((start, size)) = node_reg(node) else {
                continue;
            };
            if size == 0 {
                continue;
            }
            let Some(irq) = node_irq(node) else {
                let compatible = node.compatible().map(Compatible::first).unwrap_or("unknown");
                return Some(DeviceInfo::new(
                    device_name.to_string(),
                    start,
                    0,
                    compatible.to_string(),
                ));
            };
            let compatible = node.compatible().map(Compatible::first).unwrap_or("unknown");
            return Some(DeviceInfo::new(
                device_name.to_string(),
                start,
                irq,
                compatible.to_string(),
            ));
        }
        None
    }
}
