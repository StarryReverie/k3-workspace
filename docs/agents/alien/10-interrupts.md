# 阶段 7 实施记录：设备树与中断基础设施

日期：2026-11-15
状态：完成
范围：设备树解析、中断控制器抽象、QEMU AIA、K3 IMSIC/APLIC、K3 UART 中断
相关计划：`10-interrupts-plan.md`

## 1. 结论

阶段 7 的目标已全部达成：

- 内存、CPU、UART、IMSIC、APLIC 均从设备树读取。
- PLIC 与 IMSIC/APLIC 通过统一的中断控制器接口接入。
- QEMU `aia=aplic-imsic` 下 UART IRQ 10 由中断驱动。
- 真实 K3 上 UART IRQ 42 由中断驱动，`/proc/interrupts` 可观测。
- K3 输入路径不再是忙等轮询，读取时任务进入等待。

K3 的 UART 中断此前卡住，根因有两个，都与中断控制器无关：

```text
1. IER 的 bit6 是 XScale 型 UART 的单元使能位（UUE），
   通用 16550 写法会把它清掉，UART 停止工作。
2. MCR 的 bit3 是中断输出使能（OUT2），U-Boot 未置位，
   UART 不会拉高中断线。
```

## 2. 已完成内容

### 2.1 设备树解析

`subsystems/devices/src/prob.rs` 新增：

- `probe_memory`
- `probe_harts`
- `probe_imsic`
- `probe_aplic`

支持 `/memory@地址`、`/cpus/cpu@*`、`riscv,imsics`、`riscv,aplic` 以及
`riscv,num-ids`、`riscv,hart-index-bits`、`riscv,guest-index-bits`、
`riscv,num-sources`。

K3 启动输出：

```text
DT memory: 0x102000000..0x300000000
DT cpus:   [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
DT uart:   base=0xd4017000 irq=0 compatible=ns16550
DT imsic:  base=0xe0400000 size=0x400000 num_ids=511
           hart_bits=4 guest_bits=6
DT aplic:  base=0xe0804000 size=0x4000 num_sources=512
```

U-Boot 控制设备树的 UART 节点没有 `interrupts` 属性，因此增加平台默认值：
K3 为 42，QEMU 为 10。K3 的 42 与上游 `spacemit,k3-uart` 节点一致。

### 2.2 中断控制器抽象

`subsystems/interrupt/src/lib.rs` 增加 `IrqController::Plic` 与
`IrqController::Aia`，设备统一通过 `register_device` 注册。PLIC 行为保持不变。

外部中断入口固定为：

```text
claim -> 查设备表 -> handle_irq -> complete -> 记录计数
```

新增 `subsystems/interrupt/src/aia.rs`，实现 IMSIC 的 `eidelivery`、
`eithreshold`、`eie`，APLIC 的 `domaincfg`、`sourcecfg`、`target`、
`setienum`、`clrienum`、`clripnum`，以及 `stopei` 认领。

源模式取 `sourcecfg=6`，即 AIA 规范表 2 的 Level1（高电平触发），
对应 K3 设备树的 `IRQ_TYPE_LEVEL_HIGH`。

### 2.3 QEMU AIA

`run-k3-qemu.sh` 新增 `QEMU_AIA=aplic-imsic`，映射为
`-M virt,aia=aplic-imsic`。

参数：

```text
S-level IMSIC  0x28000000
S-level APLIC  0x0d000000
UART0 IRQ      10
```

单核与四核输出：

```text
AIA: eidelivery=0x1 eithreshold=0x0 domaincfg=0x80000104
AIA: irq 10 sourcecfg=0x6 target=0xa eie0(10)=0x400
K3 console: interrupt-driven, irq 10

1: 56
10: 94
```

### 2.4 K3 IMSIC/APLIC

参数：

```text
simsic          0xe0400000
simsic size     0x400000
saplic          0xe0804000
saplic size     0x4000
num-ids         511
num-sources     512
hart-index-bits 4
guest-index-bits 6
UART0 IRQ       42
```

初始化结果：

```text
AIA: eidelivery=0x1 eithreshold=0x0 domaincfg=0x80000104
AIA: imsic=0xe0400000 size=0x400000 ids=511 aplic=0xe0804000 size=0x4000 sources=512
AIA: hart_bits=4 guest_bits=6
AIA: irq 42 sourcecfg=0x6 target=0x2a eie0(42)=0x40000000000
```

`domaincfg` 读回为 `0x80000104`：bit31 为只读标志，bit2 表示 MSI 模式，
bit8 表示投递使能。

### 2.5 K3 UART 中断

`drivers/src/uart.rs` 增加 XScale 构造与初始化：

- `Uart8250::new_xscale` 标记 XScale 型 UART。
- 初始化时丢弃残留接收数据。
- 写 FCR 为 `0x01`：FIFO 使能、接收触发阈值 1 字节。
- 读 MCR 后置 OUT2（bit3）。
- 读 IER 后置 UUE（bit6）与 RDA（bit0）。

`devices/src/lib.rs` 让 K3 使用 `new_xscale`，并在 AIA 初始化成功且取得
中断号时统一使用 `Uart`：

```text
K3 console: interrupt UART deferred, irq 42
AIA: enabled irq 42 for hart 0
K3 console: interrupt-driven, irq 42 (IER=0x41 MCR=0xb)
```

AIA 初始化失败时仍回退 `PollingUart`。

### 2.6 上板与运行工具

- `boot-k3-board.py` 新增 `--pre-command`，可在 U-Boot 提示符下执行只读或
  低风险命令。
- `run-k3-qemu.sh` 新增 `QEMU_AIA`。

## 3. K3 UART 中断的两个根因

### 3.1 IER bit6 是单元使能位

U-Boot 下 IER 初值：

```text
md.l 0xd4017004 1
d4017004: 00000040
```

`0x40` 正是 UUE。上游 `8250_pxa` 也把该位作为单元使能位，
`serial_pxa_startup` 写入 `RLSI | RDI | RTOIE | UUE`。

Alien 依赖的 `uart8250` 库把 IER 定义为 bitflags，只包含 bit0–bit5；
`from_bits_truncate` 会丢弃 bit6，随后 `set_ier` 写回 `0x01`。
单元被关闭后，发送保持寄存器不再报告为空，`Uart8250::_put` 的等待循环
永不退出，表现为“无输出、无 trap、hart 停止推进”。

修复方式是读出 IER 后按位或上 `0x40 | 0x01`，不再让库做位域截断。
U-Boot 下的验证：

```text
mw.l 0xd4017004 0x41
md.l 0xd4017004 1
...: 00000041
echo UUE_OK
UUE_OK
```

### 3.2 MCR bit3 是中断输出使能

U-Boot 下 MCR 初值：

```text
md.l 0xd4017010 1
d4017010: 00000003
```

`0x03` 仅包含 DTR 与 RTS，OUT2 为 0。XScale 型 UART 需要 OUT2 为 1
才会拉高中断线；上游 `8250_pxa` 在 `serial_pxa_startup` 中设置
`TIOCM_OUT2`。修复后读回：

```text
K3 console: interrupt-driven, irq 42 (IER=0x41 MCR=0xb)
```

在该位被置位之前，IMSIC 与 APLIC 的配置已经正确（`eidelivery=1`、
`eie` 位已置、`domaincfg` 为 MSI 模式），但没有任何外部中断到达。
置位后立即出现 `irq 42` 计数，说明问题在 UART 一侧而不是中断控制器。

### 3.3 源模式

`sourcecfg` 采用 `6`（Level1，高电平触发）。曾短暂使用 `4`，
`4` 在 AIA 规范表 2 中是 Edge1（上升沿），与 K3 设备树的
`IRQ_TYPE_LEVEL_HIGH` 不符，已纠正。

## 4. 验证矩阵

| 环境 | 核数 | 控制台 | 结果 |
|---|---|---|---|
| QEMU 默认 PLIC | 1 | 轮询 | `nproc=1`、`K3_OK` |
| QEMU 默认 PLIC | 2 | 轮询 | `nproc=2`、`K3_OK` |
| QEMU AIA | 1 | 中断 IRQ 10 | `K3_OK` |
| QEMU AIA | 2 | 中断 IRQ 10 | `nproc=2`、`10` 计数递增 |
| QEMU AIA | 4 | 中断 IRQ 10 | `nproc=4`、`1: 56`、`10: 94` |
| QEMU AIA | 8 | 中断 IRQ 10 | 8 个 hart 均启动、`K3_OK` |
| K3 | 1 | 中断 IRQ 42 | `nproc=1`、`1: 15`、`42: 53` |
| K3 | 2 | 中断 IRQ 42 | `nproc=2`、`1: 47`、`42: 101` |
| K3 | 4 | 中断 IRQ 42 | `nproc=4`、`1: 60`、`42: 53` |
| K3 | 8 | 中断 IRQ 42 | `nproc=8`、`1: 192`、`42: 101` |

所有运行中 `0x01` 计数为 0，无 panic、无未处理 trap、无
`no device for irq`。

## 5. 修改文件

```text
subsystems/devices/src/prob.rs             设备树解析
subsystems/devices/src/lib.rs              K3 控制台与 AIA 接入
subsystems/interrupt/src/lib.rs            中断控制器抽象
subsystems/interrupt/src/aia.rs            IMSIC/APLIC 实现
subsystems/drivers/src/uart.rs             XScale UART 与中断诊断
subsystems/platform/src/k3_riscv/config.rs MMIO 与默认 IRQ
kernel/src/lib.rs                          启动顺序与陷阱入口
scripts/alien/run-k3-qemu.sh               QEMU_AIA
scripts/alien/boot-k3-board.py             --pre-command
```

## 6. 已知问题

- QEMU 八核时日志可能显示 `SMP: 6/8` 或 `7/8 harts online`。统计窗口为
  0.5 秒，超时后其余 hart 才打印 `hart N start`；实际 8 个 hart 均已启动。
  K3 八核显示 `8/8`。
- 串口抓取偶发单字节错乱，例如地址中的多余数字、`interrupt` 写成
  `interruqt`。该现象在 QEMU 上同样出现，因此位于会话抓取或输出路径，
  与 UART 中断无关。
- 内核日志走 SBI 控制台，用户态控制台直接写 UART，两条输出路径不共享
  同一把锁，可能造成字符交错。
- A100（CPU8–15）未启用。
- `sched_setaffinity` 仍未实现。

## 7. 后续

1. 让内核日志与用户态控制台共用同一 UART 输出路径，消除字符交错。
2. 进入存储阶段：QEMU virtio-blk 与 FAT，再确认 K3 的 SDHCI 或 UFS。
3. 探测并启用 A100。
4. 实现 `sched_setaffinity` 与 `taskset -p`。

## 8. 日志

- `logs/2026-11-15-k3-uart-irq.txt`：K3 单核中断控制台与 `/proc/interrupts`。
- `logs/2026-11-15-k3-uart-irq-8core.txt`：K3 八核中断控制台。
- `logs/2026-11-15-k3-uart-uue.txt`：U-Boot 下 IER、MCR 读回与内核状态。
- `logs/2026-11-15-k3-qemu-aia.txt`：QEMU AIA 四核中断 UART。
- `logs/2026-11-15-k3-aia.txt`：K3 早期轮询控制台的 AIA 初始化记录。
- `logs/2026-11-15-k3-uart-dt.txt`：U-Boot UART 设备树与寄存器查询。
