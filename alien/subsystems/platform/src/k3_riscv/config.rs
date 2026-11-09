#[cfg(feature = "k3_qemu")]
pub const CLOCK_FREQ: usize = 10_000_000;
#[cfg(not(feature = "k3_qemu"))]
pub const CLOCK_FREQ: usize = 24_000_000;
pub const BLOCK_CACHE_FRAMES: usize = 1024 * 4 * 4;
pub const HEAP_SIZE: usize = 0x200_00000;

/// K3 的外设地址空间。第一版只使用 SBI，不映射外部设备。
pub const MMIO: &[(usize, usize)] = &[];
