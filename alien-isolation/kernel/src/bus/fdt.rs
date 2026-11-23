use alloc::{string::ToString, vec::Vec};
use core::ops::Range;

use fdt::{standard_nodes::Compatible, Fdt};
use interface::AiaInfo;
use mem::PhysAddr;

use crate::bus::{CommonDeviceInfo, CommonDeviceType};

pub trait Probe {
    /// Get the base address and irq number of the uart device from the device tree.
    fn probe_uart(&self) -> Option<CommonDeviceType>;
    /// Get the base address and irq number of the rtc device from the device tree.
    fn probe_rtc(&self) -> Option<CommonDeviceType>;
    fn probe_plic(&self) -> Option<CommonDeviceType>;
    /// Get the S-level IMSIC and APLIC parameters from the device tree.
    fn probe_aia(&self) -> Option<AiaInfo>;
    /// Get the base address and irq number of the virtio devices from the device tree.
    fn probe_virtio(&self) -> Option<Vec<CommonDeviceType>>;
    fn probe_common(&self, device_name: &str, has_irq: bool) -> Option<CommonDeviceInfo>;
    fn probe_pci(&self) -> Option<CommonDeviceType>;
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
            let irq = node
                .property("interrupts")
                .map(|property| {
                    u32::from_be_bytes(property.value[0..4].try_into().unwrap())
                })
                .or_else(|| {
                    // The first pair of `interrupts-extended` is
                    // <phandle irq>.
                    node.property("interrupts-extended").and_then(|property| {
                        property
                            .value
                            .chunks_exact(4)
                            .nth(1)
                            .map(|cell| u32::from_be_bytes(cell.try_into().unwrap()))
                    })
                });
            // K3's U-Boot control DTB omits the UART interrupt specifier.
            // The SoC wires UART0 to source 42.
            #[cfg(k3)]
            let irq = irq.or(Some(::platform::config::UART_IRQ as u32));
            let first = node.compatible().map(Compatible::first).unwrap_or("");
            println!("uart fallback: {} {:#x}-{:#x} irq={:?}", first, start, end, irq);
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

    fn probe_aia(&self) -> Option<AiaInfo> {
        // QEMU and some K3 firmware revisions expose both M-level and
        // S-level AIA nodes with the same compatible strings. Select the
        // S-level IMSIC (its `interrupts-extended` includes supervisor
        // external interrupt 9), then match the APLIC whose `msi-parent`
        // points at that IMSIC.
        // (base, size, num_ids, hart_bits, guest_bits, phandle, s_level, has_index_bits)
        let mut imsics: Vec<(usize, usize, usize, u8, u8, Option<usize>, bool, bool)> = Vec::new();
        let mut aplics: Vec<(usize, usize, usize, Option<usize>)> = Vec::new();

        for node in self.all_nodes() {
            let Some(compatible) = node.compatible() else {
                continue;
            };
            let mut is_imsic = false;
            let mut is_aplic = false;
            for value in compatible.all() {
                if value.contains("imsic") {
                    is_imsic = true;
                }
                if value.contains("aplic") {
                    is_aplic = true;
                }
            }
            if !is_imsic && !is_aplic {
                continue;
            }
            let Some(reg) = node.reg().and_then(|mut reg| reg.next()) else {
                continue;
            };
            let start = reg.starting_address as usize;
            let size = reg.size.unwrap_or(if is_imsic { 0x2000 } else { 0x8000 }) as usize;
            let phandle = node
                .property("phandle")
                .and_then(|property| property.as_usize())
                .or_else(|| {
                    node.property("linux,phandle")
                        .and_then(|property| property.as_usize())
                });
            let msi_parent = node
                .property("msi-parent")
                .and_then(|property| property.as_usize());
            let supervisor = node
                .property("interrupts-extended")
                .map(|property| {
                    property
                        .value
                        .chunks_exact(4)
                        .any(|cell| u32::from_be_bytes(cell.try_into().unwrap()) == 9)
                })
                .unwrap_or(false);
            if is_imsic {
                let num_ids = node
                    .property("riscv,num-ids")
                    .and_then(|property| property.as_usize())
                    .unwrap_or(2047);
                let hart_bits = node
                    .property("riscv,hart-index-bits")
                    .and_then(|property| property.as_usize())
                    .unwrap_or(0) as u8;
                let guest_bits = node
                    .property("riscv,guest-index-bits")
                    .and_then(|property| property.as_usize())
                    .unwrap_or(0) as u8;
                let has_index_bits = hart_bits > 0 || guest_bits > 0;
                println!(
                    "AIA node: {} base={:#x} size={:#x} phandle={:?} s_level={} hart_bits={} guest_bits={}",
                    node.name, start, size, phandle, supervisor, hart_bits, guest_bits
                );
                imsics.push((
                    start,
                    size,
                    num_ids,
                    hart_bits,
                    guest_bits,
                    phandle,
                    supervisor,
                    has_index_bits,
                ));
            } else {
                println!(
                    "AIA node: {} base={:#x} size={:#x} phandle={:?} msi_parent={:?}",
                    node.name, start, size, phandle, msi_parent
                );
            }
            if is_aplic {
                let sources = node
                    .property("riscv,num-sources")
                    .and_then(|property| property.as_usize())
                    .unwrap_or(0);
                aplics.push((start, size, sources, msi_parent));
            }
        }

        // Prefer an S-level IMSIC that carries the AIA index-bit properties;
        // on K3 that is the controller actually wired to the UART. QEMU omits
        // them, so fall back to the highest S-level address.
        let imsic = imsics
            .iter()
            .filter(|entry| entry.6)
            .max_by_key(|entry| (entry.7, entry.0))
            .or_else(|| imsics.iter().max_by_key(|entry| (entry.7, entry.0)))?;
        let aplic = imsic
            .5
            .and_then(|phandle| aplics.iter().find(|entry| entry.3 == Some(phandle)))
            .or_else(|| aplics.iter().max_by_key(|entry| entry.0))?;

        let (imsic_base, imsic_size, imsic_num_ids, hart_bits, guest_bits, _, _, _) = *imsic;
        let (aplic_base, aplic_size, aplic_sources, _) = *aplic;
        println!(
            "AIA selected: imsic={:#x} aplic={:#x} imsic_phandle={:?} aplic_msi_parent={:?}",
            imsic_base, aplic_base, imsic.5, aplic.3
        );
        Some(AiaInfo {
            imsic: imsic_base..imsic_base + imsic_size,
            imsic_num_ids,
            hart_index_bits: hart_bits,
            guest_index_bits: guest_bits,
            aplic: aplic_base..aplic_base + aplic_size,
            aplic_num_sources: aplic_sources,
        })
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
        // Only generic ECAM hosts are supported in M5A. K3's DesignWare host
        // exposes DBI registers in its first `reg` entry, so a name-only match
        // would misinterpret them as an ECAM window.
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
            if !compatible
                .all()
                .any(|value| value == "pci-host-ecam-generic")
            {
                continue;
            }
            let Some(reg) = node.reg().and_then(|mut reg| reg.next()) else {
                continue;
            };
            let start = reg.starting_address as usize;
            let size = reg.size.unwrap_or(0x0010_0000) as usize;
            let first = node.compatible().map(Compatible::first).unwrap_or("");
            println!("pci: ECAM {} {:#x}-{:#x}", first, start, start + size);
            return Some(CommonDeviceType::Pci(CommonDeviceInfo {
                address_range: Range {
                    start: PhysAddr::from(start),
                    end: PhysAddr::from(start + size),
                },
                irq: None,
                compatible: Some(first.to_string()),
            }));
        }
        None
    }

    fn probe_sd(&self) -> Option<CommonDeviceType> {
        #[cfg(all(vf2, vf2_sd))]
        if let Some(device_info) = self.probe_common("sdio1", true) {
            return Some(CommonDeviceType::SdCard(device_info));
        }

        // K3 uses a standard SDHCI controller at sdh@d4280000. The driver is
        // polled, so no interrupt is required.
        #[cfg(k3)]
        for node in self.all_nodes() {
            let Some(compatible) = node.compatible() else {
                continue;
            };
            let is_sdhci = compatible
                .all()
                .any(|value| value.contains("sdhci") || value.contains("sdh"));
            if !is_sdhci {
                continue;
            }
            let Some(reg) = node.reg().and_then(|mut reg| reg.next()) else {
                continue;
            };
            let start = reg.starting_address as usize;
            let size = reg.size.unwrap_or(0x1000) as usize;
            let first = node.compatible().map(Compatible::first).unwrap_or("");
            println!("sdhci fallback: {} {:#x}-{:#x}", first, start, start + size);
            return Some(CommonDeviceType::SdCard(CommonDeviceInfo {
                address_range: Range {
                    start: PhysAddr::from(start),
                    end: PhysAddr::from(start + size),
                },
                irq: None,
                compatible: Some(first.to_string()),
            }));
        }
        None
    }
}
