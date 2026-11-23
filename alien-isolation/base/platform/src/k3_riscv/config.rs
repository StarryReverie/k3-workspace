/// K3 的时间基准频率，来自 U-Boot 控制设备树。
pub const CLOCK_FREQ: usize = 24_000_000;

/// K3 UART0 在 APLIC 上的中断源编号。
///
/// U-Boot 控制设备树的 UART 节点没有 `interrupts` 属性，因此使用平台常量。
pub const UART_IRQ: usize = 42;

/// K3 的设备地址空间。
///
/// AIA 域需要直接访问 APLIC；IMSIC 寄存器通过 CSR 访问，这里只保留地址
/// 区间用于诊断。QEMU 高地址回归使用 K3 平台配置，因此一并映射 QEMU 的
/// S-level AIA 地址。
/// BAR window used by the QEMU `virt` high-address regression.
///
/// The K3 platform configuration is also used to run the K3 image on QEMU, so
/// the QEMU PCIe ECAM and 32-bit BAR window live here. Real K3 hardware
/// assigns BARs in U-Boot and uses iATU instead.
pub const PCI_BAR_WINDOW_START: usize = 0x4000_0000;
pub const PCI_BAR_WINDOW_END: usize = 0x4010_0000;

pub const DEVICE_SPACE: &[(&str, usize, usize)] = &[
    ("uart", 0xd4017000, 0x1000),
    ("sdhci", 0xd4280000, 0x1000),
    // QEMU virt PCIe ECAM (bus 0) and 32-bit BAR window.
    ("pci-ecam-qemu", 0x3000_0000, 0x0010_0000),
    ("pci-bar-qemu", 0x4000_0000, 0x0010_0000),
    // Addresses reported by the current BPI-SM10 U-Boot control DTB.
    ("imsic-k3", 0xf1000000, 0x10000),
    ("aplic-k3", 0xf1800000, 0x4000),
    // Older K3 firmware/DTB revisions use these addresses.
    ("imsic-legacy", 0xe0400000, 0x400000),
    ("aplic-legacy", 0xe0804000, 0x4000),
    // QEMU virt UART, used by the high-address regression.
    ("uart-qemu", 0x1000_0000, 0x1000),
    ("imsic-qemu", 0x2800_0000, 0x2000),
    ("aplic-qemu", 0x0d00_0000, 0x8000),
];
