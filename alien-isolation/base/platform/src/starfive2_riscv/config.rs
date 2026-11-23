pub const CLOCK_FREQ: usize = 400_0000;

/// BAR window used by the QEMU virt host bridge; unused on VF2.
pub const PCI_BAR_WINDOW_START: usize = 0x4000_0000;
pub const PCI_BAR_WINDOW_END: usize = 0x4010_0000;

/// K3 PCIe MMIO; empty on VF2.
pub const K3_PCIE_SPACE: &[(&str, usize, usize)] = &[];

/// K3 BAR window; unused on VF2.
pub const K3_PCI_BAR_WINDOW_START: usize = 0x11_0011_0000;
pub const K3_PCI_BAR_WINDOW_END: usize = 0x11_0021_0000;

/// vf2的设备地址空间
pub const DEVICE_SPACE: &[(&str, usize, usize)] = &[
    ("rtc", 0x17040000, 0x10000),
    ("plic", 0xc000000, 0x4000000),
    ("UART", 0x10000000, 0x10000),
    ("sdio1", 0x16020000, 0x10000),
];
