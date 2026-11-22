use alloc::{string::ToString, vec::Vec};
use core::ops::Range;

use fdt::{standard_nodes::Compatible, Fdt};
use mem::PhysAddr;

use crate::bus::{CommonDeviceInfo, CommonDeviceType};

pub trait Probe {
    /// Get the base address and irq number of the uart device from the device tree.
    fn probe_uart(&self) -> Option<CommonDeviceType>;
    /// Get the base address and irq number of the rtc device from the device tree.
    fn probe_rtc(&self) -> Option<CommonDeviceType>;
    fn probe_plic(&self) -> Option<CommonDeviceType>;
    /// Get the base address and irq number of the virtio devices from the device tree.
    fn probe_virtio(&self) -> Option<Vec<CommonDeviceType>>;
    fn probe_common(&self, device_name: &str, has_irq: bool) -> Option<CommonDeviceInfo>;
    fn probe_pci(&self) -> Option<CommonDeviceType>;
    #[cfg(all(vf2, vf2_sd))]
    fn probe_sd(&self) -> Option<CommonDeviceType>;
}

impl Probe for Fdt<'_> {
    fn probe_uart(&self) -> Option<CommonDeviceType> {
        if let Some(device_info) = self.probe_common("uart", true) {
            return Some(CommonDeviceType::Uart(device_info));
        }
        if let Some(device_info) = self.probe_common("serial", true) {
            return Some(CommonDeviceType::Uart(device_info));
        }

        // K3's U-Boot control DTB does not use a `uart*` or `serial*` node
        // name. Match the known 8250/XScale compatible strings instead.
        for node in self.all_nodes() {
            if node
                .property("status")
                .and_then(|property| property.as_str())
                .map(|status| status != "okay")
                .unwrap_or(false)
            {
                continue;
            }
            let Some(compatible) = node.compatible() else {
                continue;
            };
            let matches = compatible.all().any(|value| {
                value.contains("ns16550")
                    || value.contains("8250")
                    || value.contains("xscale")
                    || value.contains("k3-uart")
            });
            if !matches {
                continue;
            }
            let Some(reg) = node.reg().and_then(|mut reg| reg.next()) else {
                continue;
            };
            let start = reg.starting_address as usize;
            let end = start + reg.size.unwrap_or(0x1000);
            let irq = node.property("interrupts").map(|property| {
                u32::from_be_bytes(property.value[0..4].try_into().unwrap())
            });
            let first = node.compatible().map(Compatible::first).unwrap_or("");
            println!("uart fallback: {} {:#x}-{:#x}", first, start, end);
            return Some(CommonDeviceType::Uart(CommonDeviceInfo {
                address_range: Range {
                    start: PhysAddr::from(start),
                    end: PhysAddr::from(end),
                },
                irq,
                compatible: Some(first.to_string()),
            }));
        }
        None
    }

    fn probe_rtc(&self) -> Option<CommonDeviceType> {
        self.probe_common("rtc", true).map(CommonDeviceType::Rtc)
    }

    fn probe_plic(&self) -> Option<CommonDeviceType> {
        if let Some(info) = self.probe_common("plic", false) {
            return Some(CommonDeviceType::Plic(info));
        }

        // QEMU names the PLIC node "interrupt-controller@...".
        for node in self.all_nodes() {
            if !node.name.starts_with("interrupt-controller@") {
                continue;
            }
            let compatible = node.compatible().map(Compatible::first).unwrap_or("");
            if !compatible.contains("plic") || compatible.contains("aplic") {
                continue;
            }
            let Some(reg) = node.reg().and_then(|mut reg| reg.next()) else {
                continue;
            };
            return Some(CommonDeviceType::Plic(CommonDeviceInfo {
                address_range: Range {
                    start: PhysAddr::from(reg.starting_address as usize),
                    end: PhysAddr::from(reg.starting_address as usize + reg.size.unwrap()),
                },
                irq: None,
                compatible: Some(compatible.to_string()),
            }));
        }
        None
    }

    fn probe_virtio(&self) -> Option<Vec<CommonDeviceType>> {
        let mut virtio_devices = Vec::new();
        for node in self.all_nodes() {
            if !node.name.starts_with("virtio") {
                continue;
            }
            if node
                .property("status")
                .and_then(|property| property.as_str())
                .map(|status| status != "okay")
                .unwrap_or(false)
            {
                continue;
            }
            {
                let reg = node.reg().unwrap().next().unwrap();
                let range = Range {
                    start: PhysAddr::from(reg.starting_address as usize),
                    end: PhysAddr::from(reg.starting_address as usize + reg.size.unwrap()),
                };
                let irq = node.property("interrupts").unwrap().value;
                let irq = u32::from_be_bytes(irq[0..4].try_into().ok().unwrap());
                let compatible = node.compatible().map(Compatible::first).unwrap();
                let device_info = CommonDeviceInfo {
                    address_range: range,
                    irq: Some(irq),
                    compatible: Some(compatible.to_string()),
                };
                virtio_devices.push(CommonDeviceType::VirtIo(device_info));
            }
        }
        Some(virtio_devices)
    }

    fn probe_common(&self, device_name: &str, has_irq: bool) -> Option<CommonDeviceInfo> {
        let node = self.all_nodes().find(|node| {
            node.name.starts_with(device_name)
                && node
                    .property("status")
                    .and_then(|property| property.as_str())
                    .map(|status| status == "okay")
                    .unwrap_or(true)
        })?;
        let reg = node.reg()?.next()?;
        let range = Range {
            start: PhysAddr::from(reg.starting_address as usize),
            end: PhysAddr::from(reg.starting_address as usize + reg.size.unwrap()),
        };
        let irq = if has_irq {
            // K3's U-Boot control DTB omits `interrupts` for the UART. Keep
            // the device usable and let the platform decide the IRQ.
            node.property("interrupts").map(|property| {
                u32::from_be_bytes(property.value[0..4].try_into().unwrap())
            })
        } else {
            None
        };
        let compatible = node.compatible().map(Compatible::first).unwrap();

        let device_info = CommonDeviceInfo {
            address_range: range,
            irq,
            compatible: Some(compatible.to_string()),
        };

        Some(device_info)
    }

    fn probe_pci(&self) -> Option<CommonDeviceType> {
        self.probe_common("pci", false).map(CommonDeviceType::Pci)
    }

    #[cfg(all(vf2, vf2_sd))]
    fn probe_sd(&self) -> Option<CommonDeviceType> {
        match self.probe_common("sdio1", true) {
            Some(device_info) => Some(CommonDeviceType::SdCard(device_info)),
            None => None,
        }
    }
}
