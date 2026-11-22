/// K3 的时间基准频率，来自 U-Boot 控制设备树。
pub const CLOCK_FREQ: usize = 24_000_000;

/// K3 的设备地址空间。
///
/// M2 只映射 UART；IMSIC、APLIC、SDHCI 与 PCIe 在后续阶段加入。
pub const DEVICE_SPACE: &[(&str, usize, usize)] = &[("uart", 0xd4017000, 0x1000)];
