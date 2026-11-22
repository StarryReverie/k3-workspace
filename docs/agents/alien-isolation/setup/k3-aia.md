# K3 AIA 中断与 UART

适用硬件：SpacemiT K3、QEMU `virt,aia=aplic-imsic`
适用系统：AlienOS `isolation` 的 K3 shell 镜像
适用工具：`nightly-2024-10-25`、U-Boot 2022.10
记录时间：2026-11-22
相关文档：[`k3-console.md`](k3-console.md)、[`../testing/k3-shell.md`](../testing/k3-shell.md)、[`../architecture/README.md`](../architecture/README.md)

本文记录 M3 的 IMSIC/APLIC 接入与中断驱动 UART。完成后 UART 输入不再依赖
轮询，`buf_uart` 在等待输入时睡眠，由 IRQ 唤醒。

## 域与 CSR 的边界

隔离域由 `DomainLoader` 直接调用入口函数，运行在内核地址空间的 S-mode，而不
是独立的 U-mode 进程。因此域可以执行 S-mode CSR，但域 crate 仍然
`#![forbid(unsafe_code)]`。

AIA 的 IMSIC 寄存器通过 `siselect`、`sireg`、`stopei` 访问，必须使用内联汇编。
这些操作放在 `domain-lib/arch/src/riscv/aia.rs`：

```text
arch::aia::imsic_read(index)
arch::aia::imsic_write(index, value)
arch::aia::imsic_claim()
```

`interrupt` 域只调用这些安全封装，不打开自己的 `unsafe`。APLIC 是普通 MMIO，
通过 `SafeIORegion` 访问。

## 接口

`PlicType` 增加 `Aia`，`PlicInfo` 增加可选的 `AiaInfo`：

```rust
pub struct AiaInfo {
    pub imsic: Range<usize>,
    pub imsic_num_ids: usize,
    pub hart_index_bits: u8,
    pub guest_index_bits: u8,
    pub aplic: Range<usize>,
    pub aplic_num_sources: usize,
}
```

`UartDomain::init` 改为接收 `UartInfo { device_info, use_irq }`。
`enable_receive_interrupt` 返回 `bool`：`true` 表示已启用 UART IRQ，`false`
表示平台没有可用中断控制器，调用方继续轮询。

## 设备树探测

内核在 `bus::init_with_dtb` 中调用 `probe_aia`，并把结果存入 `AIA_INFO`。
探测支持 `riscv,imsics`、`riscv,aplic`、`riscv,num-ids`、
`riscv,hart-index-bits`、`riscv,guest-index-bits`、`riscv,num-sources`。

QEMU 与部分 K3 固件会同时暴露 M-level 和 S-level 两套 AIA，compatible 相同。
选择规则：

1. 优先选择 `interrupts-extended` 中包含 supervisor external interrupt 9 的
   IMSIC；
2. 在 S-level 候选中优先选择带 `riscv,hart-index-bits` 或
   `riscv,guest-index-bits` 的节点；
3. 同级时取地址较高者；
4. APLIC 通过 `msi-parent` 与选中的 IMSIC phandle 匹配。

K3 板端控制 DTB 有两套 S-level 候选：

```text
IMSIC 0xf1000000  size 0x10000   hart_bits=0 guest_bits=0
APLIC 0xf1800000  size 0x4000    msi-parent -> 0xf1000000
IMSIC 0xe0400000  size 0x400000  hart_bits=4 guest_bits=6
APLIC 0xe0804000  size 0x4000    msi-parent -> 0xe0400000
```

UART 挂在后者，因此规则 2 选中 `e0400000/e0804000`。旧记录中的地址也是这一
套；`f1...` 那套在本板上不接收 UART IRQ。

QEMU 没有 index-bit 属性，S-level 只有 `0x28000000`，最终选中
`0x28000000/0x0d000000`。

## APLIC 与 IMSIC 初始化

`plic` 域的 `init` 在目标 hart 上执行：

```text
APLIC domaincfg = 0x100
IMSIC eidelivery = 0
IMSIC eie[*]    = 0
IMSIC eithreshold = 0
IMSIC eidelivery = 1
```

`register_irq` 先复位源，再配置：

```text
APLIC sourcecfg = 0
APLIC clrienum / clripnum = irq
APLIC sourcecfg = 6            高电平/边沿模式，沿用旧 K3 实现
APLIC target    = (hart << 18) | irq
APLIC setienum  = irq
IMSIC eie[irq/64] |= 1 << (irq % 64)
APLIC domaincfg = 0x104        MSI 投递 + 使能
```

K3 上 `irq=42`，`target=0x2a`，`eie` 的 bit 42 置位。`handle_irq` 通过
`stopei` 认领：

```text
value = csrrw stopei, zero
irq   = (value >> 16) & 0x7ff
```

`stopei` 的读取会清除 pending，不需要额外的 complete。

## UART IRQ 42

K3 UART 是 XScale 兼容的 8250，寄存器 32 位、`reg-shift=2`。初始化顺序：

```text
清空残留 RX 数据
FCR = 0x01          FIFO 使能，接收阈值 1 字节
MCR |= 0x08         OUT2，允许 UART 拉高中断线
IER |= 0x40         UUE，单元使能
IER |= 0x01         RDA，接收中断
```

板端实测：

```text
uart8250: IER=0x41 MCR=0xb LSR=0x0 use_irq=true
AIA: enabled irq 42 for hart 0, sourcecfg=0x6 target=0x2a
AIA/PLIC: irq 42 delivered
```

U-Boot 控制 DTB 的 UART 节点没有 `interrupts`，`probe_uart` 使用平台常量
`UART_IRQ = 42`。如果 DTB 提供 `interrupts` 或 `interrupts-extended`，优先
使用 DTB。

## 中断注册时序

`trap::init_trap_subsystem` 在域装载之前就打开了全局中断。为了不在此后的
设备初始化中触发 `PLIC_DOMAIN.get_must()` panic：

- `plic` 域在 `init` 完成后立即注册到 trap 子系统；
- 未注册 IRQ 到达时，`dispatch` 清除对应 IMSIC `eie` 位并打印
  `irq N has no device, masked`；
- 设备随后 `register_irq` 时会重新置位。

UART 先启用 RDA，`buf_uart` 稍后才注册 IRQ，因此这个遮蔽路径是必要的。

## 轮询回退

没有 AIA 节点时，内核仍使用 `PlicType::None`：

- `UartInfo.use_irq = false`；
- UART 只设置 UUE，不设置 RDA；
- `buf_uart` 在 `getc` 中直接轮询，无数据时 `yield_now`。

`run-k3-shell-qemu.sh --no-aia` 可验证该回退。

## 验证

QEMU：

```text
PASS k3-shell-1
PASS k3-shell-2
PASS k3-shell-8
```

真实 K3：

| 场景 | 关键输出 | 结果 |
|---|---|---|
| 2 核 | `AIA selected: imsic=0xe0400000 aplic=0xe0804000`、`enabled irq 42`、`delivered`、`M3_K3_OK` | 通过 |
| 8 核 | 7 个从 hart 启动、`IRQ 42` 投递、`M3_K3_OK` | 通过 |

## 已知问题

- `AIA/PLIC: irq ... delivered` 与 shell 回显可能在串口上交错，首个字符偶尔
  看起来被覆盖；命令实际完整执行。
- K3 不同固件版本的 AIA 地址不同，`DEVICE_SPACE` 同时映射
  `0xe0...` 与 `0xf1...` 两套地址。
- `AIA: delivered` 只在每个 IRQ 第一次投递时打印，用于验收，不用于计数。
- `/proc/interrupts` 尚未接入 `irq_info`。
