# AlienOS 与 K3 移植笔记

日期：2026-11-09
范围：AlienOS 的架构与启动依赖、K3 平台事实、最小移植方案
不涉及：开发板实测、外设驱动实现

## 1. AlienOS 概述

Alien 是用 Rust 实现的 RISC-V 操作系统。内核运行在 S 模式，依赖更上层的 M 模式固件提供 SBI 接口。它不是 Linux 兼容内核，用户态只实现部分 Linux 系统调用。

设计上强调模块拆分：页表、调度器、VFS、网络、锁等组件以独立 crate 引入。仓库顶层包含 `kernel/`、`subsystems/`、`user/`、`tests/` 与 `tools/`。

## 2. 已支持的平台

| 平台 | 启动方式 | 说明 |
|---|---|---|
| QEMU `virt` | `-kernel` 直接加载 | 当前基线，完整可用 |
| VisionFive2（JH7110） | U-Boot TFTP，链接地址 `0x40200000` | 内嵌 DTB 与 initramfs，使用 SDIO 块设备 |
| SiFive Unmatched（FU740） | U-Boot TFTP | 同类 SBI 启动方式 |

仓库没有 K3 平台代码。

## 3. 启动依赖

- 控制台、定时器与关机使用旧版 SBI 调用：`console_putchar`、`console_getchar`、`set_timer`、`shutdown`。
- 机器信息从 DTB 解析：内存范围、PLIC、CLINT、initrd、bootargs。
- 入口 `_start` 从 `a0` 读取 hartid，从 `a1` 读取 DTB 地址。
- 初始用户进程固定为 `/tests/init`。
- 默认从 FAT 磁盘 `/dev/sda` 挂载 `/tests`，initramfs 只填充 `/bin` 与 `/sbin`。
- VisionFive2 平台额外要求 SDIO，并在 DTB 中查找 RTC、UART 与 virtio 设备。

## 4. K3 平台事实

### 4.1 来自本仓库串口日志

来源：`logs/session-20261103-100023.log`，BananaPi-1。

```text
U-Boot 2022.10
CPU: rv64imafdcvh
DRAM: 8 GiB
SBI specification v2.0
SBI implementation ID=0x1 Version=0x10006
SBI TIME / IPI / RFENCE / SRST / DBCN extension detected
Kernel moved to 0x102200000
DTB at 0x138000000
```

Linux 可见内存范围：

```text
node 0: [mem 0x0000000102000000-0x00000002ffffffff]
```

保留区：

```text
0x100000000..0x101ffffff
```

启动地址：

| 用途 | 地址 |
|---|---|
| 内核加载 | `0x102200000` |
| DTB | `0x110000000` 或 `0x138000000`，随启动方式变化 |
| FIT 加载 | `0x140000000` |
| `bootm` | `0x140000000` |

### 4.2 来自公开资料

- K3 包含 8 个 X100 通用核与 8 个 A100 AI 核。
- Linux 会枚举 16 个 hart，A100 也以 CPU 形式出现。
- 支持 RVV 1.0、Hypervisor 扩展与 RVA23。
- 板载 8、16 或 32 GiB LPDDR5。
- 主线 Linux 已加入 K3 SoC 与部分板型设备树，可作为 UART、PLIC、时间基准的参考。

公开资料只用于确定方向；具体地址与中断号仍以板端设备树和实测为准。

## 5. 最小移植方案

以下方案已在阶段 1 实现，构建与静态验收见 `04-k3-minimal.md`。第一版只验证 SBI、内存管理与用户态加载，不接磁盘、网卡与外部中断。

### 5.1 平台模块

新增 `k3` feature 与 `subsystems/platform/src/k3_riscv/`：

- 内存范围固定为 `0x102000000..0x300000000`。
- 内核链接地址 `0x102200000`。
- 控制台使用 SBI DBCN。
- 定时器使用 SBI TIME。
- 关机使用 SBI SRST。
- 不解析 DTB，不初始化 PLIC。
- 不启动其他 hart，第一版保持单核。

### 5.2 内核侧改动

- `kernel/src/lib.rs`：为 `k3` 跳过 `devices::init_device()` 与 `interrupt::init_plic()`。
- `subsystems/vfs/src/lib.rs`：为 `k3` 跳过 `/dev/sda` 与 `/tests` 的磁盘挂载。
- `subsystems/vfs/src/initrd.rs`：允许从 initramfs 解出任意路径，而不只 `bin/` 与 `sbin/`。
- `kernel/src/task/mod.rs`：初始进程仍读取 `/tests/init`，由 initramfs 提供。
- `kernel/src/ksym.rs`：kallsyms 缺失时跳过，已由补丁 0002 完成。

### 5.3 构建与镜像

- 新增 `tools/linker-k3.ld`，`BASE_ADDRESS = 0x102200000`。
- 使用 `rust-objcopy` 或 `llvm-objcopy` 生成 raw bin。
- 使用 FIT 镜像并通过 `bootm` 启动，确保 U-Boot 传递 hartid 与 DTB。
- 不使用裸 `go`，因为无法保证 `a0` 与 `a1` 的取值。

### 5.4 启动流程

```text
ostool board connect
→ Ctrl+C 停在 U-Boot
→ tftpboot 0x140000000 alien-k3.itb
→ bootm 0x140000000
```

## 6. 未确认事项

| 项目 | 影响 | 处理方式 |
|---|---|---|
| 时间基准频率 | `CLOCK_FREQ` 错误会导致时间与调度 tick 不准 | 读取 K3 设备树 `timebase-frequency`，或用 `rdtime` 与墙钟比对 |
| 中断控制器 | K3 可能使用 PLIC、APLIC 或 IMSIC | 查设备树与 Linux 启动日志；第一版不使用外部中断 |
| UART 基址与中断号 | 影响交互输入 | 查设备树；必要时改用 SBI DBCN 轮询 |
| 设备树来源 | 影响内存、UART、PLIC 解析 | 主线 K3 DTS 或板端 `/sys/firmware/fdt` |
| OpenSBI 旧版 console | 若未启用，`console_putchar` 无输出 | 直接改用 SBI DBCN |
| FIT 的 `load` 与 `entry` | 错误会导致跳转失败 | 生成后检查 FIT 属性与反汇编 |
| 内存保留区 | 可能覆盖固件或 RCPU 区域 | 第一版只用 `0x102200000` 之后的内存 |

## 7. 风险

- 串口输入不可靠，启动早期日志与输入可能互相干扰。
- 平台会话约 5 分钟回收，首次启动可能需要多次尝试。
- 若完全没有输出，区分“未跳转”“SBI 调用失败”“页表异常”需要提前埋入早期标记。
- K3 的 16 个 hart 中 A100 的启动方式与 X100 可能不同，SMP 不能直接照搬 VisionFive2。
- 设备树缺失时只能硬编码，后续增加外设需要重新引入 DTB。

## 8. 参考

- Alien 仓库：<https://github.com/Godones/Alien>
- Alien 文档：<https://godones.github.io/Alien/>
- 本仓库平台与启动记录：`docs/agents/01-platform.md`、`docs/agents/02-uboot-cheatsheet.md`
- 本仓库 K3 实测：`docs/agents/04-k3-findings.md`
