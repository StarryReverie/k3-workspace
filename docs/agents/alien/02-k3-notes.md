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

阶段 3 在 U-Boot 中读取控制设备树，确认了以下事实：

| 项目 | 值 |
|---|---|
| `timebase-frequency` | `0x016e3600`，即 24 MHz |
| UART | `ns16550`，基址 `0xd4017000`，长度 `0x100`，`clock-frequency = 0x00e10000` |
| 控制台 | `stdout-path = "uart0:115200"` |
| APLIC | `0xf1800000`、`0xe0804000`，均为 `status = "disabled"` |
| IMSIC | `0xf1000000`、`0xe0400000` |
| 内存节点 | `0x100000000..0x300000000` |
| `fdtcontroladdr` | 每次启动变化，本次为 `0x2fbe903c0` |

因此 K3 的外部中断路径应基于 IMSIC，不能按 PLIC 处理。第一版仍未初始化中断控制器。

### 4.2 来自公开资料

- K3 包含 8 个 X100 通用核与 8 个 A100 AI 核。
- Linux 会枚举 16 个 hart，A100 也以 CPU 形式出现。
- 支持 RVV 1.0、Hypervisor 扩展与 RVA23。
- 板载 8、16 或 32 GiB LPDDR5。
- 主线 Linux 已加入 K3 SoC 与部分板型设备树，可作为 UART、IMSIC/APLIC 与时间基准的参考。

公开资料只用于确定方向；具体地址与中断号仍以板端设备树和实测为准。

## 5. 最小移植方案

以下方案已在阶段 1 实现，构建与静态验收见 `04-k3-minimal.md`。第一版只验证 SBI、内存管理与用户态加载，不接磁盘、网卡与外部中断。

### 5.1 平台模块

新增 `k3` feature 与 `subsystems/platform/src/k3_riscv/`：

- 内存范围固定为 `0x102000000..0x300000000`。
- 内核链接地址 `0x102200000`。
- 内核日志使用 SBI DBCN。
- 用户态 `/dev/tty` 使用轮询 UART：K3 为 `Uart8250@0xd4017000`，QEMU 为 `Uart16550@0x10000000`。
- 定时器使用 SBI TIME。
- 关机使用 SBI SRST。
- 不解析 DTB，不初始化 PLIC、APLIC 或 IMSIC。
- 不启动其他 hart，第一版保持单核。

### 5.2 内核侧改动

- `kernel/src/lib.rs`：为 `k3` 跳过 `devices::init_device()` 与 `interrupt::init_plic()`，并注册轮询 UART；主 hart 由 `platform::boot_hart()` 固定，次 hart 等待 `INIT_DONE`。
- `subsystems/devices/src/uart.rs`：新增 `PollingUart`，不依赖中断。
- `subsystems/platform/src/lib.rs`：记录启动 hart，启动前用 HSM 查询 hart 状态。
- `subsystems/vfs/src/lib.rs`：为 `k3` 跳过 `/dev/sda` 与 `/tests` 的磁盘挂载。
- `subsystems/vfs/src/initrd.rs`：允许从 initramfs 解出任意路径，而不只 `bin/` 与 `sbin/`。
- `kernel/src/task/mod.rs`：初始进程仍读取 `/tests/init`，由 initramfs 提供。
- `kernel/src/ksym.rs`：kallsyms 缺失时跳过，已由本地修改完成。

### 5.3 构建与镜像

- 新增 `tools/linker-k3.ld`，`BASE_ADDRESS = 0x102200000`。
- 使用 `rust-objcopy` 或 `llvm-objcopy` 生成 raw bin。
- 使用 FIT 镜像并通过 `bootm` 启动，确保 U-Boot 传递 hartid 与 DTB。
- FIT 使用 gzip 压缩，U-Boot 解压后加载到 `0x102200000`。
- FIT 当前不含 FDT 节点，启动命令必须带 `$fdtcontroladdr` 作为第三个参数。
- 不使用裸 `go`，因为无法保证 `a0` 与 `a1` 的取值。
- 多核构建使用 `K3_CPUS=2|4|8`，构建脚本通过 `SMP=<核数>` 设置 `CPU_NUM`。

### 5.4 启动流程

```text
ostool board connect
→ Ctrl+C 停在 U-Boot
→ tftpboot 0x140000000 alien-k3.itb
→ bootm 0x140000000 - $fdtcontroladdr
```

当前 FIT 不含 FDT 节点。若省略第三个参数，U-Boot 会报
`Device tree not found or missing FDT support` 并复位开发板。

## 6. 已确认与待确认事项

已确认：

| 项目 | 结论 |
|---|---|
| 时间基准频率 | 24 MHz，`CLOCK_FREQ` 不需要修改 |
| 中断控制器 | `simsic` 位于 `0xe0400000`，`saplic` 位于 `0xe0804000` |
| UART | 基址 `0xd4017000`，控制台 `uart0:115200`，`reg-shift=2`、32 位寄存器 |
| UART 类型 | `spacemit,k3-uart` / `intel,xscale-uart`，属 XScale 型 |
| UART 单元使能 | IER bit6 为 UUE，写 IER 必须保留该位 |
| UART 中断输出 | MCR bit3 为 OUT2，必须置位才会拉高中断线 |
| UART 中断号 | 42，高电平触发；U-Boot 控制设备树未提供该属性 |
| 设备树来源 | U-Boot 控制设备树，地址由 `fdtcontroladdr` 给出 |
| FIT 与 `bootm` | FIT 不含 FDT 时必须显式传入 FDT 地址 |
| 高地址路径 | 真实硬件与 10 GiB QEMU 回归均可用 |
| 控制台交互 | 轮询 UART 已替代 SBI 输入，完整 bash 可交互 |
| `0x01` 洪泛 | 来自 SBI getchar 轮询，已消除 |
| FIT gzip | 5.0 MiB 压缩到 4.0 MiB，U-Boot 解压正常 |
| SMP | X100 的 CPU0–7 可启动，4 核与 8 核已验证 |
| CPU 亲和性 | 默认掩码为全部 CPU，`nproc` 与 `/sys` 输出正确 |
| 控制台输出 | 内核日志与用户态 UART 共用输出锁 |
| QEMU AIA | `aia=aplic-imsic` 下 UART IRQ 10 中断驱动已可用 |
| K3 AIA | IMSIC/APLIC 初始化成功，UART IRQ 42 中断驱动已可用 |
| K3 UART 中断 | 单核、双核、四核、八核均已验证 |

仍需确认：

| 项目 | 影响 | 处理方式 |
|---|---|---|
| A100（CPU8–15） | 尚未探测与启用 | 先用 `hart_get_status` 查询 8–15，再单独实验 |
| 调度器亲和性 | `sched_setaffinity` 返回 `ENOSYS` | 在任务选择路径检查 `cpu_affinity` |
| 串口字符交错 | 内核走 SBI 控制台，用户态直接写 UART | 让两条输出路径共用同一 UART |
| QEMU 八核在线统计 | 0.5 秒窗口内可能只统计到 6/8 或 7/8 | 延长统计窗口或改为事件计数 |
| U-Boot DHCP | 偶发 `ipaddr not set` | 上板脚本增加 DHCP 重试 |
| UART 流控与作业控制 | Ctrl-C、流控未验证 | 补 termios 与信号处理 |
| 长时间计时 | 调度精度未知 | 运行定时器与 `nanosleep` 测试 |
| 上传速度 | 仍受宿主机网络影响 | 继续压缩或改变传输方式 |

## 7. 风险

- 内核日志与用户态控制台是两条独立输出路径，字符可能交错。
- 平台会话约 5 分钟回收，完整镜像上传可能接近上限。
- 若完全没有输出，区分“未跳转”“SBI 调用失败”“页表异常”需要提前埋入早期标记。
- K3 的 16 个 hart 中 A100 的启动方式与 X100 可能不同，SMP 不能直接照搬 VisionFive2。
- 多 hart 会放大调度器竞态；任务上下文必须在保存完成后才能发布到其他 hart。
- 设备树缺失时只能硬编码，后续增加外设需要重新引入 DTB。

## 8. 参考

- Alien 仓库：<https://github.com/Godones/Alien>
- Alien 文档：<https://godones.github.io/Alien/>
- 本仓库平台与启动记录：`docs/agents/01-platform.md`、`docs/agents/02-uboot-cheatsheet.md`
- 本仓库 K3 实测：`docs/agents/04-k3-findings.md`
