# K3 控制台与 shell

适用硬件：SpacemiT K3、QEMU `virt`
适用系统：AlienOS `isolation`，提交见 [`../README.md`](../README.md)
适用工具：`nightly-2024-10-25`、U-Boot 2022.10
记录时间：2026-11-22
相关文档：[`k3-platform.md`](k3-platform.md)、[`../testing/k3-shell.md`](../testing/k3-shell.md)、[`../architecture/README.md`](../architecture/README.md)

本文记录 K3 上从诊断内核到可交互 shell 的改动。控制台使用轮询 UART，
IMSIC/APLIC 中断路径尚未启用。

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

## 轮询 UART

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

不设置 IER bit0，因此不产生接收中断。`buf_uart` 在 `IS_K3` 下直接轮询
`uart.getc()`，在无输入时 `yield_now`，而不是等待 IRQ 唤醒。

QEMU 上的 `uart16550` 域在 K3 构建中同样跳过 `enable_receive_interrupt`，
保持轮询路径一致。

## 中断控制器占位

K3 没有 PLIC，但域装载接口仍要求一个 `PLICDomain`。新增
`PlicType::None`：

- 内核用零长度范围创建 `plic` 域；
- 域初始化只打印 `plic disabled`，不访问硬件；
- `register_irq` 与 `handle_irq` 直接返回；
- 外部中断保持关闭。

这是 M3 的控制台阶段方案。IMSIC/APLIC 接入后会用 AIA 替换该占位。

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

K3 没有块设备，VFS 调整如下：

- 跳过 `fatfs-1` 与 `/dev/sda` 挂载；
- 根文件系统使用 ramfs；
- initramfs 解出 `bin/`、`sbin/` 与 `tests/`；
- devfs 不创建 `/dev/sda`；
- `/tests/init` 与 `/tests/bash` 来自 initramfs。

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
- 次 hart 等待 `STARTED`，主 hart 完成域装载后清除。

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

- 未初始化 IMSIC/APLIC，UART 输入与定时器仍由轮询或 SBI 处理。
- 内核日志走 SBI，用户态控制台走 UART MMIO，两条路径可能交错。
- 无块设备、SDHCI、网络与 PCIe。
- K3 的 8 核 shell 可以到达提示符；板端输入验证目前以 2 核为主。
- 域仍按 QEMU/k3 两套 cfg 构建，尚未形成通用的平台抽象。
