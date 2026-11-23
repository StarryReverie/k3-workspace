pub const CLOCK_FREQ: usize = 1250_0000;

/// BAR window used when the QEMU virt host bridge is enumerated.
pub const PCI_BAR_WINDOW_START: usize = 0x4000_0000;
pub const PCI_BAR_WINDOW_END: usize = 0x4010_0000;

/// K3 PCIe MMIO; empty on other platforms.
pub const K3_PCIE_SPACE: &[(&str, usize, usize)] = &[];

/// K3 BAR window; unused on this platform.
pub const K3_PCI_BAR_WINDOW_START: usize = 0x11_0011_0000;
pub const K3_PCI_BAR_WINDOW_END: usize = 0x11_0021_0000;

/// qemu的设备地址空间
pub const DEVICE_SPACE: &[(&str, usize, usize)] = &[
    ("test", 0x10_0000, 0x1000),
    ("rtc", 0x10_1000, 0x1000),
    ("clint", 0x200_0000, 0x10000),
    ("plic", 0xc00_0000, 0x600000),
    ("virtio-mmio", 0x1000_0000, 0x9000),
    ("pci-ecam", 0x3000_0000, 0x0010_0000),
    ("pci-bar", 0x4000_0000, 0x0010_0000),
];
