# 阶段 7 实施记录：设备树与中断基础设施

日期：2026-11-15
状态：部分完成
范围：设备树解析、中断控制器抽象、QEMU AIA、K3 IMSIC/APLIC 初始化
未完成：K3 UART 中断使能
相关计划：`10-interrupts-plan.md`

## 1. 结论

阶段 7 已完成基础设施与 QEMU 验证：

- K3 与 QEMU 的内存、CPU、UART、IMSIC、APLIC 可以从设备树读取。
- PLIC 与 IMSIC/APLIC 通过统一的中断控制器接口接入。
- QEMU `aia=aplic-imsic` 下，UART IRQ 10 由中断驱动，`/proc/interrupts` 可观测。
- 真实 K3 上 `simsic` 与 `saplic` 可以探测并完成初始化。

K3 的 UART 中断仍未完成：

- 写 UART IER 后主 hart 停止推进。
- 当前 K3 回退到轮询 UART。
- QEMU 与 K3 的 AIA 初始化路径共用，QEMU 已验证，K3 的 UART 具体实现仍需进一步确认。

## 2. 已完成内容

### 2.1 设备树解析

`subsystems/devices/src/prob.rs` 新增：

- `probe_memory`
- `probe_harts`
- `probe_imsic`
- `probe_aplic`

支持：

- `/memory@地址` 节点。
- `/cpus/cpu@*` 的 hart 编号。
- `riscv,imsics` 与 `riscv,aplic`。
- `riscv,num-ids`、`riscv,hart-index-bits`、`riscv,guest-index-bits`、`riscv,num-sources`。

K3 启动输出：

```text
DT memory: 0x102000000..0x300000000
DT cpus:   [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
DT uart:   base=0xd4017000 irq=0 compatible=ns16550
DT imsic:  base=0xe0400000 size=0x400000 num_ids=511
           hart_bits=4 guest_bits=6
DT aplic:  base=0xe0804000 size=0x4000 num_sources=512
```

说明：

- U-Boot 控制设备树的 UART 节点是 `/soc/uart@d4017000`，没有 `interrupts` 属性。
- 因此增加平台默认 UART IRQ：K3 为 42，QEMU 为 10。
- U-Boot 控制设备树枚举了 16 个 CPU，但当前内核只启动配置的 hart 数。

### 2.2 中断控制器抽象

`subsystems/interrupt/src/lib.rs` 增加：

```text
IrqController::Plic
IrqController::Aia
```

设备注册统一为 `register_device`，PLIC 行为保持不变。

外部中断入口改为：

```text
claim -> 查设备表 -> handle_irq -> complete -> 记录计数
```

新增 `subsystems/interrupt/src/aia.rs`：

- IMSIC `eidelivery`、`eithreshold`、`eie`。
- APLIC `domaincfg`、`sourcecfg`、`target`、`setienum`、`clrienum`、`clripnum`。
- `stopei` claim。
- RV64 的偶数 `eie` 选择号处理。

### 2.3 QEMU AIA

`run-k3-qemu.sh` 新增：

```bash
QEMU_AIA=aplic-imsic
```

映射为：

```bash
-M virt,aia=aplic-imsic
```

QEMU AIA 参数：

```text
S-level IMSIC  0x28000000
S-level APLIC  0x0d000000
UART0 IRQ      10
```

验证结果：

```text
AIA: imsic=0x28000000 size=0x2000 ids=255
AIA: aplic=0xd000000 size=0x8000 sources=96
AIA: enabled irq 10 for hart 0
K3 console: interrupt-driven, irq 10
```

用户态：

```text
/bin/busybox nproc
2

/bin/busybox cat /proc/interrupts
1: 30
10: 6

echo K3_OK
K3_OK
```

说明 IMSIC、APLIC、UART IRQ 与中断处理链路在 QEMU 上完整可用。

### 2.4 K3 IMSIC/APLIC 初始化

K3 参数：

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
AIA: imsic=0xe0400000 size=0x400000 ids=511
AIA: aplic=0xe0804000 size=0x4000 sources=512
AIA: hart_bits=4 guest_bits=6
K3 console: polling
SMP: 2/2 harts online
```

K3 上可以执行：

```text
/bin/busybox nproc
2

echo K3_OK
K3_OK

/bin/busybox ls /
bin    dev    etc    proc   root   sys    tests  tmp    var
```

无 trap，`0x01` 数量为 0。

### 2.5 上板工具

`boot-k3-board.py` 新增 `--pre-command`，可在 U-Boot 提示符后、上传前发送只读命令。

用于确认：

```text
fdt print /aliases
fdt print /soc/uart@d4017000
md.l 0xd4017000 8
```

## 3. K3 UART 中断阻塞

### 3.1 现象

K3 上使能 UART IER 后，主 hart 停止推进：

```text
K3 console: enable UART IER before APLIC source
Uart: enable irq
<无后续输出>
```

无 panic，无 trap。

### 3.2 已确认的硬件事实

U-Boot 控制设备树 UART 节点：

```text
uart@d4017000 {
    compatible = "ns16550";
    reg = <0x0 0xd4017000 0x0 0x100>;
    reg-shift = <2>;
    reg-io-width = <4>;
    status = "okay";
};
```

U-Boot 下读取寄存器正常：

```text
md.l 0xd4017000 8
d4017000: 00000000 00000040 000000c1 00000003
```

其中 `0xd4017004` 为 IER，初始值 `0x40`。

### 3.3 已尝试的方案

- 32 位写 IER。
- 8 位写 IER。
- 延迟到陷阱子系统就绪后再写 IER。
- 先写 IER，再配置 APLIC 源。
- 关闭 APLIC 全局投递后再写 IER。
- 清空 IMSIC `eie`。
- 清空 UART RX FIFO。
- 绕过 `Uart` 锁，直接写 `0xd4017004`。

以上仍会卡住。

### 3.4 当前判断

问题更可能位于以下方向之一：

- K3 `intel,xscale-uart` 的 IER 访问特性。
- UART IRQ 42 与 APLIC/IMSIC 的实际路由。
- 固件对 UART 或 APLIC 的预配置与 S-mode 访问限制。

QEMU AIA 证明抽象层与 IMSIC/APLIC 代码路径本身可用，阻塞集中在 K3 UART 硬件细节。

### 3.5 当前策略

```text
QEMU 默认 PLIC   -> PollingUart
QEMU AIA         -> 中断驱动 Uart
K3               -> PollingUart，同时初始化 AIA
```

K3 的 AIA 初始化保持全局投递关闭，不注册 UART 设备，因此不会触发未处理中断。

## 4. 验证矩阵

| 环境 | UART 模式 | 结果 |
|---|---|---|
| QEMU 默认 PLIC | 轮询 | `Alien:/#` |
| QEMU AIA 双核 | 中断 | `nproc=2`、IRQ 10 计数 6 |
| K3 双核 | 轮询 | `nproc=2`、`K3_OK`、`ls` |
| K3 双核 AIA | 轮询 | AIA 初始化成功，IRQ 42 不计数 |

## 5. 修改文件

```text
subsystems/devices/src/prob.rs          设备树解析
subsystems/devices/src/lib.rs           K3 控制台与 AIA 接入
subsystems/interrupt/src/lib.rs         中断控制器抽象
subsystems/interrupt/src/aia.rs         IMSIC/APLIC 实现
subsystems/drivers/src/uart.rs          延迟 UART 初始化
subsystems/platform/src/k3_riscv/config.rs  MMIO 与默认 IRQ
kernel/src/lib.rs                       启动顺序与陷阱入口
scripts/alien/run-k3-qemu.sh            QEMU_AIA
scripts/alien/boot-k3-board.py          --pre-command
```

## 6. 已知问题

- K3 UART IER 使能后卡住，K3 仍使用轮询控制台。
- K3 `/proc/interrupts` 没有 IRQ 42 计数。
- A100（CPU8–15）未启用。
- `sched_setaffinity` 仍未实现。
- UART 中断若启用，需要确认中断与 APLIC 的实际路由。

## 7. 下一步

1. 对照 Linux `8250_pxa` 与 K3 厂商 UART 驱动，确认 IER 访问方式。
2. 在 U-Boot 下执行 `mw.l 0xd4017004 1`，验证 M-mode 写入 IER 后系统是否正常。
3. 检查 UART IRQ 42 在 APLIC 中的实际 source 与 target 配置。
4. 若短期无法解决，保留 K3 轮询控制台，转向存储阶段。

## 8. 日志

- `logs/2026-11-15-k3-qemu-aia.txt`：QEMU AIA 中断 UART 与 `/proc/interrupts`。
- `logs/2026-11-15-k3-aia.txt`：K3 AIA 初始化与轮询控制台。
- `logs/2026-11-15-k3-uart-dt.txt`：K3 U-Boot UART 设备树与寄存器查询。
