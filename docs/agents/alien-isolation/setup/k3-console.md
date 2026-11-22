# K3 控制台与 shell

适用硬件：SpacemiT K3、QEMU `virt`
适用系统：AlienOS `isolation`，提交见 [`../README.md`](../README.md)
适用工具：`nightly-2024-10-25`、U-Boot 2022.10
记录时间：2026-11-22
相关文档：[`k3-platform.md`](k3-platform.md)、[`../testing/k3-shell.md`](../testing/k3-shell.md)、[`../architecture/README.md`](../architecture/README.md)

本文记录 K3 上从诊断内核到可交互 shell 的改动。控制台最初使用轮询 UART，
M3 完成后由 IMSIC/APLIC 中断驱动，详见 [`k3-aia.md`](k3-aia.md)。

## 平台判断

`cfg(k3)` 只由发出该 cfg 的 crate 自己看到：

| crate | cfg 来源 |
|---|---|
| `kernel` | `kernel/build.rs` |
| `base/platform` | `base/platform/build.rs` |
| `domain-lib/config` | `domain-lib/config/build.rs` |
| 其他域 crate | 无 |

域 crate 没有各自的 `build.rs`，因此域内的 `#[cfg(k3)]` 不会生效。域侧统一
使用 `config::IS_K3` 常量：

```rust
pub const IS_K3: bool = cfg!(k3);
```

`PLATFORM=k3` 构建域时，`config` 的 `build.rs` 发出 `cfg(k3)`，该常量随之为
`true`。域构建必须带 `PLATFORM=k3`，否则会退回 QEMU 行为。

## UART 与中断模式

K3 UART 是 XScale 兼容的 8250，寄存器为 32 位、`reg-shift=2`。

设备选择：

| 平台 | compatible | 域 |
|---|---|---|
| QEMU `virt` | `ns16550a` | `uart16550` |
| K3 | `ns16550` 或 `k3-uart` | `uart8250` |

K3 的 U-Boot 控制设备树没有 `interrupts` 属性。FDT 探测允许 IRQ 缺失，并在
节点名不匹配 `uart`/`serial` 时按 compatible 扫描兜底。

`uart8250` 域在 K3 下执行 XScale 初始化：

```text
FCR = 0x01           FIFO 使能，接收阈值 1 字节
MCR |= 0x08          OUT2，拉高中断输出
IER |= 0x40          UUE，单元使能
```

有 AIA 时再置 IER bit0（`0x41`），`buf_uart` 等待 IRQ 唤醒；没有 AIA 时只
保留 UUE，`buf_uart` 直接轮询 `uart.getc()`，无输入时 `yield_now`。

QEMU 上的 `uart16550` 域使用同一套判断：`UartInfo.use_irq` 为真时打开接收
中断，否则保持轮询。平台判断不再写死在域内。

## 中断控制器

K3 没有 PLIC，域装载接口仍要求一个 `PLICDomain`。M3 增加
`PlicType::Aia`：

- 内核从设备树探测 IMSIC/APLIC，把 `AiaInfo` 传给 `plic` 域；
- `plic` 域配置 APLIC 和当前 hart 的 IMSIC S-file；
- 外部中断通过 `stopei` 认领，再分发给设备域。

没有 AIA 节点时回退到 `PlicType::None`：域初始化只打印 `plic disabled`，
`register_irq` 与 `handle_irq` 直接返回，UART 保持轮询。AIA 细节见
[`k3-aia.md`](k3-aia.md)。

## 设备集合

K3 的 FDT 探测做以下调整：

| 设备 | 处理 |
|---|---|
| UART | 按 compatible 兜底识别 |
| PLIC | 不存在，使用占位域 |
| PCIe | M5 之前跳过 |
| virtio-mmio | 跳过 |
| RTC | 跳过 |
| loopback | 添加假设备，满足 `net_stack` 与 `vfs` |

跳过 virtio 是必要的：`register_mmio_device` 会读取 MMIO magic，而 K3 上
没有 virtio；QEMU 高地址回归的空白槽位也会在 trap 初始化前触发异常。

## 根文件系统

M3 阶段 K3 还没有块设备，VFS 以 ramfs 为根：

- 没有 `cache_blk-1` 时跳过 FAT 与 `/dev/sda`；
- initramfs 解出 `bin/`、`sbin/` 与 `tests/`；
- `/tests/init` 与 `/tests/bash` 来自 initramfs。

M4 增加 SDHCI 后，有可用卡时块栈会创建 `cache_blk-1`，devfs 创建
`/dev/sda`，FAT 挂载到 `/mnt`；无卡时仍走上面的回退路径。细节见
[`k3-storage.md`](k3-storage.md)。

initramfs 通过内核特性 `k3-initrd` 嵌入内核 `.rodata`。U-Boot 控制设备树
不可靠地携带 ramdisk，因此 shell 镜像不依赖 FIT ramdisk。构建顺序必须是：

```text
生成 initramfs
-> 复制到 build/k3-initramfs.cpio.gz
-> 编译内核（include_bytes!）
-> 生成 FIT
```

## 多核启动

上游 `main` 使用 `STARTED` 的 `compare_exchange` 选举主 hart。次 hart 在
`platform_init` 中先于 `main` 启动，可能抢到主 hart 分支。K3/10 GiB QEMU
暴露了该竞态。

修复方式：

- `platform_init` 在启动次 hart 前记录 `BOOT_HART`；
- `main` 只允许该 hart 执行主路径；
- 新增 `BOOT_DONE`，主 hart 完成内存/页表/域装载后置位；
- 次 hart 用 `while !BOOT_DONE` 等待，再开启分页与 trap。

此前的等待条件写成 `while STARTED`，而 `STARTED` 在次 hart 到达时通常仍为
`false`，次 hart 会直接执行 `init_memory_system`。8 核 QEMU 上曾因此出现
`alloc 1 frame failed` 并关机。

## 域构建工具

`cargo domain` 原先忽略子 `cargo build` 的退出码。域编译失败时会复制旧产物并
继续，造成“构建成功但运行旧代码”。现在子构建失败立即退出。

## 构建与运行

```bash
nix develop

# 构建完整 shell 镜像
./scripts/alien-isolation/build-k3.sh --mode shell --smp 8

# QEMU 高地址运行
./scripts/alien-isolation/run-k3-shell-qemu.sh --smp 8

# 自动测试
./scripts/alien-isolation/test-k3-shell.py --build all
```

产物位于 `alien-isolation/build/k3-shell/smp-N/`：

```text
kernel-k3              嵌入 initramfs 的 ELF
kernel-k3.bin.gz       FIT 使用的压缩内核
initramfs.cpio.gz      同一份 initramfs
alien-k3-shell.itb     U-Boot FIT
```

## 已知限制

- 定时器中断仍由 SBI/CLINT 路径处理，AIA 只接管外部中断。
- 内核日志走 SBI，用户态控制台走 UART MMIO，两条路径可能交错。
- 存储见 [`k3-storage.md`](k3-storage.md)；网络与 PCIe 尚未接入。
- 域仍按 QEMU/k3 两套 cfg 构建，尚未形成通用的平台抽象。
- K3 不同固件版本的 AIA 地址不同，平台同时映射两套地址。
