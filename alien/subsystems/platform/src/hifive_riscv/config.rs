pub const CLOCK_FREQ: usize = 100_0000;
/// 磁盘文件系统的挂载点。
pub const DISK_MOUNT: &str = "/tests";
pub const BLOCK_CACHE_FRAMES: usize = 1024 * 4 * 4;
pub const HEAP_SIZE: usize = 0x70_00000; //64M

pub const MMIO: &[(usize, usize)] = &[
    (0xc000000, 0x4000000), //PLIC
];
