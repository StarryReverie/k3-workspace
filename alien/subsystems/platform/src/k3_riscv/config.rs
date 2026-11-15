#[cfg(feature = "k3_qemu")]
pub const CLOCK_FREQ: usize = 10_000_000;
#[cfg(not(feature = "k3_qemu"))]
pub const CLOCK_FREQ: usize = 24_000_000;

/// UART0 中断号。U-Boot 的控制设备树可能省略该属性。
#[cfg(feature = "k3_qemu")]
pub const UART_IRQ: usize = 10;
#[cfg(not(feature = "k3_qemu"))]
pub const UART_IRQ: usize = 42;
pub const BLOCK_CACHE_FRAMES: usize = 1024 * 4 * 4;
pub const HEAP_SIZE: usize = 0x200_00000;

/// K3 的外设地址空间。
#[cfg(feature = "k3_qemu")]
pub const MMIO: &[(usize, usize)] = &[
    (0x1000_0000, 0x1000),   // UART0
    (0x1000_1000, 0x8000),   // virtio-mmio 槽位
    (0x0d00_0000, 0x1_0000), // QEMU AIA S-level APLIC
    (0x2800_0000, 0x1_0000), // QEMU AIA S-level IMSIC
];
#[cfg(not(feature = "k3_qemu"))]
pub const MMIO: &[(usize, usize)] = &[
    (0xd401_7000, 0x1000),    // UART0
    (0xe040_0000, 0x40_0000), // K3 simsic
    (0xe080_0000, 0x1_0000),  // K3 saplic
];
