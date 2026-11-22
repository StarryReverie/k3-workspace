# 隔离内核启动与平台接口

适用硬件：QEMU `virt`、SpacemiT K3
适用系统：AlienOS `isolation`，提交见 [`../README.md`](../README.md)
适用工具：`nightly-2024-10-25`、`cargo domain build-all`
记录时间：2026-11-22
相关文档：[`../setup/qemu-baseline.md`](../setup/qemu-baseline.md)、[`../setup/k3-platform.md`](../setup/k3-platform.md)、[`../testing/qemu-matrix.md`](../testing/qemu-matrix.md)

本文记录阅读隔离树源码后得到的启动路径与平台接口结论，用于后续 K3 存储、
中断与 PCIe 移植。内容以源码为准，不重复各阶段的操作步骤。

## 可信基与域的边界

隔离树把内核与用户态服务拆成两类实体：

| 部分 | 位置 | 职责 |
|---|---|---|
| 可信基（TCB） | `kernel/`、`base/` | 启动、分页、trap、任务上下文、域加载、共享内存 |
| 平台层 | `base/platform/` | DTB、SBI、串口输出、定时器、关机 |
| 域 | `domains/` | 调度、VFS、文件系统、块设备、UART、网络、日志 |
| 域公共库 | `domain-lib/` | 域运行时、共享堆、系统调用代理、同步 |

域通过系统调用与内核交互。设备域不能使用 `unsafe`，因此旧宏内核的驱动不能
直接复制。平台差异集中在 TCB 与域构建配置两处，驱动域本身不应感知板型。

## 启动顺序

内核入口 `_start` 位于 `base/platform/src/common_riscv/boot.rs`。汇编把 `a0`
作为 hart id、`a1` 作为 DTB 地址写入 `tp` 与 `gp`，切换到按 hart 划分的启动
栈，然后调用 `platform_init`。

`platform_init` 依次执行：

1. 清空 `.bss`。
2. 打印启动标志。
3. 保存 DTB 指针。
4. 从 DTB 构造 `MachineInfo`。
5. 初始化日志。
6. 通过 SBI HSM 启动其余 hart。
7. 进入 `main`。

主 hart 在 `kernel/src/main.rs` 中继续：

```text
打印 Boot hart 与 MachineInfo
初始化帧分配器，重定位 initrd
建立内核页表并开启分页
扫描 DTB 注册平台设备
初始化 trap
装载全部域
开始调度
```

从 hart 在 `BOOT_DONE` 上自旋，等主 hart 完成内存、页表与域装载后再开启分页、
初始化 trap，并进入调度。

主 hart 不能靠 `STARTED` 的 `compare_exchange` 选举。`platform_init` 会在
进入 `main` 之前启动从 hart，从 hart 可能先到达比较操作并抢到主路径。
`platform_init` 现在先记录 `BOOT_HART`，`main` 只允许该 hart 初始化 TCB。

`BOOT_DONE` 初始为 `false`。此前的等待条件写成 `while STARTED`，而 `STARTED`
在从 hart 到达时通常也是 `false`，从 hart 会直接执行 `init_memory_system`，
抢先访问尚未建立的页表与帧分配器。8 核 QEMU 上表现为
`alloc 1 frame failed` 后关机。

## 平台选择

平台由构建时的 `PLATFORM` 环境变量决定，默认 `qemu_riscv`。

| 位置 | 作用 |
|---|---|
| `domain-lib/config/build.rs` | 根据 `PLATFORM` 发出 `cfg(qemu_riscv)`、`cfg(vf2)` 或 `cfg(k3)` |
| `base/platform/src/lib.rs` | 按 cfg 选择平台模块与 `config` 导出 |
| `kernel/build.rs` | 选择链接基址，`k3` 为 `0x102200000`，`vf2` 为 `0x40200000` |

`domain-lib/config` 与 `base/platform` 各自维护一份 `CLOCK_FREQ`：

- 内核的定时器与 `base/mem` 使用 `base/platform` 的配置。
- 域内的时间接口使用 `domain-lib/config` 的配置。

两者必须一致。K3 均为 24 MHz。

域构建必须传 `PLATFORM=k3`，否则域配置按 QEMU 的 12.5 MHz 编译，时间换算
会偏差约一倍。

`cfg(k3)` 只对发出该 cfg 的 crate 生效。域 crate 没有自己的 `build.rs`，
因此域侧不使用 `#[cfg(k3)]`，而是读取 `config::IS_K3` 常量。

## 内存模型

链接脚本 `tools/link.ld` 定义 `BASE_ADDRESS`、`skernel`、`sheap` 等符号。
`kernel/build.rs` 在构建时复制脚本并替换基址。

帧分配器起点为：

```text
frame_start = sheap + KERNEL_HEAP_SIZE
```

`KERNEL_HEAP_SIZE` 为 64 MiB。页表把 `sheap..memory_end` 作为内核空闲区映射。

| 平台 | 链接地址 | RAM 范围 | 说明 |
|---|---|---|---|
| QEMU 基线 | `0x80200000` | `0x80000000..` | `-kernel` 直接加载 |
| K3 | `0x102200000` | `0x102000000..0x300000000` | U-Boot FIT 加载 |
| K3 高地址 QEMU | `0x102200000` | `0x80000000..0x300000000` | `-m 10G` 覆盖 K3 上界 |

K3 的 `0x100000000..0x101ffffff` 为保留区。内核从 `0x102200000` 开始，帧
分配器从内核末尾之后开始，因此不会落入保留区。

## DTB 与机器信息

`base/platform/src/common_riscv/basic.rs` 只解析少量字段：

| 字段 | 来源 | K3 实测 |
|---|---|---|
| `smp` | `/cpus` 子节点数量 | 16，包含 8 个 X100 与 8 个 A100 |
| `memory` | `memory` 节点的 `reg` | `0x102000000..0x300000000`，U-Boot 已扣除保留区 |
| `plic` | 节点名以 `plic` 开头 | 不存在 |
| `clint` | 节点名以 `clint` 开头 | 解析到 `0xf1810000..0xf1820000`，不用于中断 |
| `initrd` | `/chosen` 的 `linux,initrd-start/end` | 由启动方式决定 |
| `bootargs` | `/chosen/bootargs` | 可为空 |

K3 使用 IMSIC 与 APLIC，不使用 PLIC。`MachineInfo.plic` 在 K3 上为 `0..0`。
`clint` 字段会匹配到 `0xf1810000..0xf1820000`，但该节点不是 K3 外部中断的
使用入口。依赖 PLIC 的初始化不能在 K3 上直接运行。

`initrd` 只有在启动器填充 `/chosen` 时才存在。QEMU 的 `-initrd` 可以填充；
K3 的 U-Boot 控制设备树不会可靠地携带 FIT ramdisk，因此 K3 shell 镜像通过
`k3-initrd` 特性把 initramfs 嵌入内核，并在 `basic_machine_info` 中覆盖
`initrd`。

设备探测会跳过 `status != "okay"` 的节点。K3 的 UART 节点没有 `interrupts`
属性，节点名也不一定是 `uart*` 或 `serial*`，因此 `probe_uart` 还会按
`ns16550`、`8250`、`xscale`、`k3-uart` 扫描 compatible。K3 不探测 PCIe 与
virtio：virtio-mmio 注册会读取 MMIO，而该操作发生在 trap 初始化之前。

## 域装载的前置条件

`kernel/src/domain/mod.rs` 的 `load_domains` 并不是“能启动多少算多少”，它按
固定顺序创建域，任一前置条件缺失都会失败：

1. `init_device()` 在 QEMU/VF2 上要求平台设备中存在名为 `plic` 的设备；K3
   改为从 DTB 读取 AIA 参数，没有 AIA 时使用 `PlicType::None`。
2. `plic` 域的初始化传入 `PlicType` 与可选 `AiaInfo`。K3 的 AIA 域在
   S-mode 运行，IMSIC CSR 通过 `arch::aia` 安全封装访问。
3. `vfs` 域初始化时调用 `basic::get_domain("net_stack").unwrap()`，因此没有
   网络域时 VFS 无法启动。
4. `net_stack` 初始化要求存在 `nic` 设备域。
5. `task` 域固定读取 `/tests/init`，initramfs 或磁盘必须提供该路径。
6. `uart16550` 与 `virtio_mmio_block` 在 `qemu_riscv` 下加入 init 域集合。

这些依赖解释了为什么 K3 不能直接复用 QEMU 的域集合：K3 没有 PLIC、没有
virtio，UART 也不是 16550 兼容的编程模型。

## K3 的差异

| 项目 | QEMU `virt` | SpacemiT K3 |
|---|---|---|
| 外部中断 | PLIC | IMSIC + APLIC |
| 控制台 | SBI 或 16550 | SBI 输出，XScale 型 UART |
| 块设备 | virtio-mmio | SDHCI、UFS、NVMe |
| 启动方式 | `-kernel` 与 `-initrd` | U-Boot FIT 与 `bootm` |
| 链接地址 | `0x80200000` | `0x102200000` |
| 时间基准 | 12.5 MHz | 24 MHz |
| 设备树 | QEMU 自动生成 | U-Boot 控制设备树，地址随启动变化 |

M2 因此引入 `k3-diag`：只验证 DTB、内存、分页、initrd 重定位与从 hart
启动，不进入设备探测与域装载。这样可以在不引入 AIA 与 XScale UART 的前提下
确认 TCB 在高地址与 8 核配置下可用。

## 已确认的构建约束

- `SMP` 在编译期决定内核的 `CPU_NUM`，每个核数是独立的内核二进制。
- 域侧按 hart 索引的数组使用固定 `MAX_CPU_NUM = 8`，域产物与核数解耦。
- `PLATFORM`、`KERNEL_BASE`、`SMP`、`LOG` 等构建变量必须通过
  `rerun-if-env-changed` 跟踪，否则 Cargo 会复用旧产物。
- 启动 hart 不固定。高地址 QEMU 的 8 核运行中出现过启动 hart 为 1 的情况。

## K3 中断与控制台的域调整

| 位置 | 调整 |
|---|---|
| `plic` 域 | `PlicType::Aia` 时配置 APLIC 与 IMSIC；无 AIA 时 `PlicType::None` 只占位 |
| `uart8250` 域 | K3 下按 XScale 初始化，FCR/MCR/IER 处理 FIFO、OUT2、UUE 与 RDA |
| `uart16550` 域 | 按 `UartInfo.use_irq` 决定是否打开接收中断 |
| `buf_uart` 域 | `use_irq` 为真时等待 IRQ 唤醒，否则直接轮询 |
| `vfs` 域 | K3 跳过 FAT 与 `/dev/sda`，根文件系统来自 ramfs 与 initramfs |
| `devfs` 域 | K3 不创建 `/dev/sda` |
| 设备表 | K3 添加 loopback，满足 `net_stack` 与 `vfs` 对 NIC 的依赖 |
| 域构建工具 | 子 `cargo build` 失败时立即退出，不再复制旧产物 |

`cargo domain` 原先会忽略子构建失败，导致“构建成功但运行旧 ELF”。该问题在
PLIC 域上实际出现过。

域由 `DomainLoader` 直接调用入口函数，运行在内核地址空间的 S-mode，而不是
U-mode 进程。这是 AIA 域能够执行 `siselect`/`sireg`/`stopei` 的前提；域 crate
仍通过 `arch::aia` 保持 `forbid(unsafe_code)`。

K3 板端 DTB 同时暴露两套 S-level IMSIC/APLIC。内核优先选择带
`riscv,hart-index-bits`/`riscv,guest-index-bits` 的 IMSIC，并用 APLIC 的
`msi-parent` 匹配其 phandle。板端最终选中 UART 所在的
`0xe0400000/0xe0804000`；QEMU 选中 `0x28000000/0x0d000000`。

## 对后续阶段的含义

| 阶段 | 需要在 TCB 或平台层完成的工作 |
|---|---|
| M3 中断与控制台 | 完成：AIA 初始化、UART IRQ 与中断驱动 shell；板端 2/8 核通过 |
| M4 存储 | SDHCI 块设备域；`/tests` 从 SD 卡挂载；域构建切换到 `PLATFORM=k3` |
| M5 PCIe/NVMe | K3 主机桥与 PHY 初始化；ECAM 映射；NVMe 块设备域 |
| M6 收尾 | 域集合的平台化；K3 上完整 shell 与多核回归 |
