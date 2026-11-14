#[cfg(feature = "k3_qemu")]
pub const CLOCK_FREQ: usize = 10_000_000;
#[cfg(not(feature = "k3_qemu"))]
pub const CLOCK_FREQ: usize = 24_000_000;
pub const BLOCK_CACHE_FRAMES: usize = 1024 * 4 * 4;
pub const HEAP_SIZE: usize = 0x200_00000;

/// K3 的外设地址空间。第一版只映射轮询 UART。
#[cfg(feature = "k3_qemu")]
pub const MMIO: &[(usize, usize)] = &[(0x1000_0000, 0x1000)];
#[cfg(not(feature = "k3_qemu"))]
pub const MMIO: &[(usize, usize)] = &[(0xd401_7000, 0x1000)];
