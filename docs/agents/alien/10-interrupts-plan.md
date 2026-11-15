# 阶段 7 详细计划：设备树与中断基础设施

日期：2026-11-15
状态：部分实施，进展见 `10-interrupts.md`
前置提交：`3b2db72 docs: add stage 7 interrupt plan`
目标平台：QEMU `virt` 与 BananaPi BPI-SM10（SpacemiT K3）

## 0. 文档范围

本文件只描述阶段 7 的实施计划，不代表已经实施。

阶段 7 的目标是让 K3 具备可用的外部中断，并把 UART 输入从忙等轮询切换为中断驱动。阶段 7 不包含存储、网络、A100 与完整设备树探测。

## 1. 工作量判断

### 1.1 与阶段 5、6 的对比

| 项目 | 阶段 5 | 阶段 6 | 阶段 7 |
|---|---|---|---|
| 主要目标 | gzip FIT、双核 | 四核/八核 | 设备树、IMSIC/APLIC、UART 中断 |
| 代码改动 | 约 15 个文件，约 170 行 | 15 个文件，168 行新增、57 行删除 | 预计 20–25 个文件，500–900 行 |
| 新增子系统 | 无 | 无 | IMSIC/APLIC、设备树探测、中断抽象 |
| 主要风险 | 主 hart 竞态 | 任务上下文发布竞态 | AIA 寄存器、MSI 地址、硬件调试 |
| 验证环境 | QEMU + 板卡 | QEMU + 板卡 | QEMU PLIC + QEMU AIA + 板卡 |
| 预计工作量 | 1 个工作单元 | 1–2 个工作单元 | 4–6 个工作单元 |

结论：阶段 7 的代码量约为阶段 6 的 3–5 倍，调试不确定性更高。主要不确定性来自 K3 的 APLIC MSI 地址配置与 IMSIC hart 文件布局，而不是 Rust 代码本身。

### 1.2 为什么仍然建议先做阶段 7

- UART 输入当前是忙等轮询，占用 CPU。
- 存储与网络后续都需要外部中断。
- 设备树解析同时为 A100、SD/UFS、网络与后续外设提供基础。
- QEMU 11.1.2 支持 `aia=aplic-imsic`，可以先在 QEMU 验证大部分逻辑。

## 2. 当前状态

### 2.1 中断层

- `subsystems/interrupt/src/lib.rs` 只有 PLIC 实现。
- `external_interrupt_handler` 直接调用 PLIC 的 claim/complete。
- `register_device_to_plic` 绑定 PLIC 与当前 hart。
- K3 的 `init_plic` 直接跳过。

### 2.2 设备树

- `devices::init_device` 使用 `fdt` crate 探测 RTC、UART、virtio。
- K3 完全跳过 `init_device`。
- `probe_common` 可以读取 UART 基址与 `interrupts`，但没有 IMSIC/APLIC 探测。
- 平台信息仍以 `k3_riscv/mod.rs` 的硬编码为主。

### 2.3 UART

- K3 使用 `PollingUart`。
- `drivers::uart::Uart` 已经实现中断接收、`rx_buf`、等待队列与任务唤醒。
- `Uart8250::_init` 已使能 RDA 中断。
- K3 尚未把 UART IRQ 注册到任何中断控制器。

### 2.4 QEMU AIA

QEMU 11.1.2 支持：

```text
-M virt,aia=aplic-imsic
```

QEMU 生成的设备树中：

| 对象 | 地址 | 说明 |
|---|---|---|
| S-level APLIC | `0x0d000000`，大小 `0x8000` | `qemu,aplic` / `riscv,aplic` |
| S-level IMSIC | `0x28000000`，2 hart 时大小 `0x2000` | `qemu,imsics` / `riscv,imsics` |
| UART0 | `0x10000000` | 中断号 10，电平高 |
| APLIC `num-sources` | `0x60` | 96 个源 |
| IMSIC `num-ids` | `0xff` | 255 个身份 |

### 2.5 K3 AIA

| 对象 | 地址 | 说明 |
|---|---|---|
| `simsic` | `0xe0400000`，大小 `0x400000`（U-Boot DT） | S-level IMSIC，枚举 16 个 hart |
| `saplic` | `0xe0804000`，大小 `0x4000` | S-level APLIC |
| UART0 | `0xd4017000` | 中断号 42，`reg-shift=2` |
| `simsic` 属性 | `hart-index-bits=4`、`guest-index-bits=6`、`num-ids=511` | 每个 hart 64 页 |
| `saplic` 属性 | `num-sources=512`、`msi-parent=<&simsic>` | 源 42 为 UART0 |
| `mimsic` / `maplic` | `0xf1000000` / `0xf1800000` | `reserved`，不使用 |

K3 的 U-Boot 控制设备树枚举了 CPU0–15，`simsic` 覆盖 `0x400000`。当前内核只启动 `CPU_NUM` 指定的 hart。A100 不在阶段 7 范围内。

## 3. 目标与非目标

### 3.1 目标

1. 解析阶段 7 必需的最小设备树信息。
2. 引入中断控制器抽象，保持 PLIC 路径不回归。
3. 在 QEMU AIA 上实现 IMSIC/APLIC 与 UART 中断。
4. 在 K3 上实现 IMSIC/APLIC 与 UART 中断。
5. 保留轮询 UART 作为早期控制台与回退路径。
6. 在单核、四核、八核上验证。

### 3.2 非目标

- 完整设备树设备探测。
- SD/UFS 存储。
- 以太网。
- A100 与 16 核。
- `sched_setaffinity` 强制生效。
- UART 流控、Ctrl-C 与完整作业控制。

## 4. 设计决策

### 4.1 运行时检测 AIA 与 PLIC

不增加大量 feature 组合，而是在启动时检查设备树：

```text
存在 riscv,imsics 节点 -> 初始化 IMSIC/APLIC
否则                  -> 初始化 PLIC
```

这样：

- QEMU 默认 `-M virt` 继续走 PLIC。
- QEMU `aia=aplic-imsic` 走 AIA。
- K3 走 AIA。
- 同一份 `k3_qemu` 代码可以覆盖两种 QEMU 配置。

### 4.2 保留轮询 UART

UART 中断初始化成功前，仍使用 `PollingUart` 输出早期日志。

中断可用后：

- 输入切换到 `Uart` + `Uart8250`。
- 输出仍可使用轮询或中断，阶段 7 不强制改输出路径。
- 保留环境变量或 feature 回退到纯轮询模式，便于定位问题。

### 4.3 UART 中断只绑定一个 hart

TTY 是共享设备，阶段 7 只让一个 hart 处理 UART 中断：

- 优先使用 `platform::boot_hart()`。
- APLIC 的 `target[42]` 指向该 hart 的 IMSIC S-file。
- 其他 hart 只初始化自己的 IMSIC，不使能 UART 身份。
- `Uart::handle_irq` 唤醒的任务可以被其他 hart 窃取，调度器已有窃取逻辑。

### 4.4 IMSIC 文件地址

IMSIC S-file 地址按 AIA 规范计算：

```text
s_file(hart) = imsic_base + (hart << (12 + guest_index_bits))
```

- QEMU 没有 `guest-index-bits`，按 0 处理，步长为 `0x1000`。
- K3 `guest-index-bits=6`，步长为 `0x40000`。
- K3 16 个 hart 覆盖 `0x400000`，与 U-Boot 设备树的 `simsic` 大小一致。

### 4.5 APLIC MSI 目标

APLIC 在 MSI 模式下，`target[irq]` 格式为：

```text
bits 31:18  Hart Index
bits 17:12  Guest Index
bits 10:0   EIID
```

阶段 7 使用：

```text
target[irq] = (hart_index << 18) | (0 << 12) | irq
```

- QEMU UART IRQ 10。
- K3 UART IRQ 42。
- 只有 target hart 接收 UART 中断。

### 4.6 IMSIC 配置通过 CSR

IMSIC 的 4 KiB MMIO 页只包含 MSI 写入端口：

```text
0x000  seteipnum_le
0x004  seteipnum_be
```

配置通过 `siselect` / `sireg` CSR：

| `siselect` | 寄存器 | 用途 |
|---|---|---|
| `0x70` | `eidelivery` | 使能 IMSIC 投递，写 1 |
| `0x72` | `eithreshold` | 阈值，写 0 |
| `0x80 + n` | `eip` | pending 位 |
| `0xC0 + n` | `eie` | enable 位 |

CSR 编号：

```text
siselect = 0x150
sireg    = 0x151
stopei   = 0x15C
```

claim 使用 `csrrw rd, stopei, x0`，读取后写入零即可清除对应 pending 位。

### 4.7 APLIC 配置

APLIC 寄存器偏移：

| 偏移 | 寄存器 |
|---|---|
| `0x0000` | `domaincfg` |
| `0x0004 + 4*(i-1)` | `sourcecfg[i]` |
| `0x1BC8` / `0x1BCC` | `smsiaddrcfg` / `smsiaddrcfgh` |
| `0x1CDC` | `setipnum` |
| `0x1EDC` | `setienum` |
| `0x1FDC` | `clrienum` |
| `0x3004 + 4*(i-1)` | `target[i]` |

阶段 7 配置：

```text
domaincfg = 0x104        # BE=0, DM=1 (MSI), IE=1
sourcecfg[irq] = 6       # Level1，UART 为电平高
target[irq] = ...
setienum = irq
```

### 4.8 `smsiaddrcfg` 处理

如果 APLIC 没有硬连线 MSI 地址，需要配置 `smsiaddrcfg` / `smsiaddrcfgh`。

实施时：

1. 读取 `smsiaddrcfg` 与 `smsiaddrcfgh`。
2. 检查锁定位与 `L` 位。
3. 如果未锁定且值为零，按 AIA 规范写入 IMSIC 基址 PPN 与 hart 位移。
4. 如果已锁定，直接使用固件配置。
5. 打印读回值，便于上板定位。

不在计划中假定 K3 固件一定配置好，也不假定一定为空。

## 5. 工作包

### 7A：最小设备树解析

**目标**

解析 UART、内存、hart、IMSIC、APLIC，只打印，不改变启动路径。

**修改文件**

- `alien/subsystems/devices/src/prob.rs`
- `alien/subsystems/devices/src/lib.rs`
- `alien/kernel/src/lib.rs`

**新增探测接口**

```rust
pub struct AiaInfo {
    pub imsic_base: usize,
    pub imsic_size: usize,
    pub imsic_num_ids: usize,
    pub hart_index_bits: u8,
    pub guest_index_bits: u8,
    pub aplic_base: usize,
    pub aplic_size: usize,
    pub aplic_num_sources: usize,
}

pub trait Probe {
    fn probe_memory(&self) -> Option<Range<usize>>;
    fn probe_harts(&self) -> Vec<usize>;
    fn probe_imsic(&self) -> Option<AiaInfo>;
    fn probe_aplic(&self) -> Option<AiaInfo>;
}
```

**实现要点**

- 匹配 `riscv,imsics`、`qemu,imsics`。
- 匹配 `riscv,aplic`、`qemu,aplic`。
- 读取 `reg`、`riscv,num-ids`、`riscv,hart-index-bits`、`riscv,guest-index-bits`、`riscv,num-sources`。
- 读取 `/cpus/cpu@*` 的 `reg`。
- 读取 `/memory@*` 的 `reg`。
- QEMU 的 `reg` 地址单元为 2，大小单元为 2。
- K3 同样为 2/2。

**K3 启动调用点**

```text
platform_init
  -> main(boot hart)
  -> mem::init_memory_system
  -> devices::init_k3_uart
  -> devices::probe_k3_platform
  -> vfs::init_filesystem
```

`probe_k3_platform` 只打印，不初始化设备。

**验收**

```text
Memory: 0x102000000..0x300000000
CPUs:   0..7
UART:   0xd4017000 irq 42 reg-shift 2
IMSIC:  0xe0400000 size 0x400000 num-ids 511
APLIC:  0xe0804000 size 0x4000 num-sources 512
```

### 7B：中断控制器抽象

**目标**

让 `external_interrupt_handler` 与设备注册不再直接依赖 PLIC。

**修改文件**

- `alien/subsystems/interrupt/src/lib.rs`
- `alien/subsystems/interrupt/src/plic.rs`（可新增）
- `alien/subsystems/interrupt/src/aia.rs`（可新增）
- `alien/subsystems/devices/src/lib.rs`
- `alien/subsystems/interrupt/Cargo.toml`（如拆模块）

**建议接口**

```rust
pub trait IrqController: Send + Sync {
    fn register(&self, irq: usize, device: Arc<dyn DeviceBase>);
    fn claim(&self) -> Option<usize>;
    fn complete(&self, irq: usize);
}
```

或者使用枚举：

```rust
pub enum IrqController {
    Plic(PlicController),
    Aia(AiaController),
}
```

枚举方式避免 trait object 与动态分配，优先采用。

**寄存器与设备表**

- 保留 `DEVICE_TABLE` 与 `INTERRUPT_RECORD`。
- `register_device(irq, device)` 代替 `register_device_to_plic`。
- `external_interrupt_handler`：

```text
irq = controller.claim()
device = DEVICE_TABLE[irq]
device.handle_irq()
controller.complete(irq)
record_irq(irq)
```

**PLIC 兼容**

- `init_plic` 保留。
- PLIC 的 `register/claim/complete` 移入 `PlicController`。
- QEMU 默认基线必须不回归。

**验收**

```text
QEMU 基线 Alien:/#
k3_qemu 诊断 K3_QEMU_INIT_OK
```

### 7C：QEMU AIA 原型

**目标**

在 QEMU `aia=aplic-imsic` 下完成 IMSIC/APLIC 初始化和 UART 中断。

**修改文件**

- `alien/subsystems/interrupt/src/aia.rs`
- `alien/kernel/src/lib.rs`
- `alien/subsystems/devices/src/lib.rs`
- `scripts/alien/run-k3-qemu.sh`

**运行参数**

```bash
QEMU_AIA=aplic-imsic \
K3_CPUS=4 K3_QEMU_SHELL=1 \
./scripts/alien/build-k3-qemu.sh

QEMU_AIA=aplic-imsic QEMU_SMP=4 QEMU_TIMEOUT=15 \
./scripts/alien/run-k3-qemu.sh
```

`run-k3-qemu.sh` 增加：

```text
QEMU_AIA 为空       -> -M virt
QEMU_AIA=aplic-imsic -> -M virt,aia=aplic-imsic
```

**AIA 初始化顺序**

```text
1. 解析 DT，取得 IMSIC/APLIC/UART IRQ
2. 配置 APLIC domaincfg
3. 配置 sourcecfg[irq] = Level1
4. 配置 target[irq] 指向目标 hart
5. 使能 setienum = irq
6. 在目标 hart 上配置 IMSIC
   - eidelivery = 1
   - eithreshold = 0
   - eie0 bit irq = 1
7. 使能 sie.SEIE
8. 注册 UART 设备
```

**FDT 探测细节**

- QEMU AIA 的 S-level APLIC 是 `interrupt-controller@d000000`，`msi-parent` 指向 `interrupt-controller@28000000`。
- 只使用 `riscv,imsics` 且 `interrupts-extended` 指向 `cpu_intc 9`（S-mode external）的节点。
- 不使用 M-level 的 `0x24000000` IMSIC 与 `0xc000000` APLIC。

**验收**

```text
QEMU AIA 单核启动
QEMU AIA 四核/八核启动
UART 输入由中断驱动
bash 可交互
0x01 数量为 0
```

### 7D：K3 IMSIC/APLIC

**目标**

在真实 K3 上配置 `simsic` 与 `saplic`，让 UART0 中断号 42 可用。

**参数**

```text
simsic base       0xe0400000
simsic size       0x400000
hart index bits   4
guest index bits  6
num ids           511
saplic base       0xe0804000
saplic size       0x4000
num sources       512
UART IRQ          42
UART base         0xd4017000
```

**S-file 地址**

```text
s_file(hart) = 0xe0400000 + (hart << 18)
```

示例：

```text
hart 0: 0xe0400000
hart 1: 0xe0404000
hart 2: 0xe0408000
...
hart 7: 0xe041c000
```

**APLIC 寄存器地址**

```text
domaincfg     0xe0804000
sourcecfg[42] 0xe08040a8
target[42]    0xe08070a8
setienum      0xe0805edc
smsiaddrcfg   0xe0805bc8
smsiaddrcfgh  0xe0805bcc
```

**初始化顺序**

```text
1. 读取 smsiaddrcfg/smsiaddrcfgh，打印锁定状态
2. 必要时写入 IMSIC 基址与 hart 位移
3. domaincfg = 0x104
4. sourcecfg[42] = 6 (Level1)
5. target[42] = (boot_hart << 18) | 42
6. setienum = 42
7. 在 boot_hart 上配置 IMSIC S-file
   - eidelivery = 1
   - eithreshold = 0
   - eie0 bit 42 = 1
8. 使能 sie.SEIE
9. 注册 UART 设备
```

**调试输出**

```text
AIA: imsic=0xe0400000 aplic=0xe0804000 irq=42
AIA: smsiaddrcfg=... locked=...
AIA: s_file=0xe0400000
AIA: domaincfg=0x104 sourcecfg=6 target=...
AIA: interrupt count=N
```

**风险**

- K3 的 `smsiaddrcfg` 可能由固件锁定或硬连线。
- `saplic` 的源 42 可能被固件配置为其他模式。
- `simsic` 的 S-file 地址公式需要上板读回验证。
- UART 中断可能与其他设备共享 APLIC 源。

**验收**

```text
K3 单核：UART 中断计数递增，bash 可输入
K3 四核：nproc=4，UART 中断正常
K3 八核：nproc=8，UART 中断正常
0x01 数量为 0
无 trap
```

### 7E：UART 中断驱动

**目标**

把 K3 的输入路径从 `PollingUart` 切换到 `Uart`。

**修改文件**

- `alien/subsystems/devices/src/lib.rs`
- `alien/subsystems/devices/src/uart.rs`
- `alien/kernel/src/lib.rs`

**实现要点**

- `init_k3_uart` 增加参数：是否使用中断。
- AIA 初始化成功后：
  - 使用 `Uart8250` 低层驱动。
  - 使用 `Uart` 包装器。
  - 注册 UART IRQ。
- AIA 不可用或初始化失败：
  - 回退 `PollingUart`。
- 保留 `UART_DEVICE` 单例。
- 输出路径可以继续使用轮询或 SBI，阶段 7 不强制统一。

**验收**

```text
无输入时 CPU 不忙等
UART 输入正常
回显正常
中断计数递增
```

### 7F：回归与文档

**回归矩阵**

| 环境 | 期望 |
|---|---|
| QEMU 默认 PLIC | `Alien:/#` |
| QEMU AIA 单核 | UART 中断、shell |
| QEMU AIA 四核 | 4/4 hart、`nproc=4` |
| QEMU AIA 八核 | 8/8 hart、`nproc=8` |
| K3 单核 | UART 中断、shell |
| K3 四核 | 4/4 hart、`nproc=4` |
| K3 八核 | 8/8 hart、`nproc=8` |

**文档**

- 新增 `docs/agents/alien/10-interrupts.md`
- 更新 `README.md`
- 更新 `02-k3-notes.md`
- 更新 `03-progress.md`
- 新增日志节选

**提交拆分**

```text
feat: parse K3 device tree for interrupt discovery
feat: add IMSIC and APLIC interrupt controller
feat: use interrupt-driven UART on K3
docs: record K3 interrupt infrastructure
```

## 6. 关键寄存器参考

### 6.1 IMSIC CSR

| `siselect` | 寄存器 | 说明 |
|---|---|---|
| `0x70` | `eidelivery` | 1 = 使能 IMSIC 投递 |
| `0x72` | `eithreshold` | 0 = 不屏蔽 |
| `0x80 + n` | `eip` | pending |
| `0xC0 + n` | `eie` | enable |

- `siselect` = 0x150
- `sireg` = 0x151
- `stopei` = 0x15C
- 身份 `id` 对应 `eie[id/64]` 的 `id%64` 位。

### 6.2 APLIC

| 偏移 | 寄存器 |
|---|---|
| `0x0000` | `domaincfg` |
| `0x0004 + 4*(i-1)` | `sourcecfg[i]` |
| `0x1BC8` | `smsiaddrcfg` |
| `0x1BCC` | `smsiaddrcfgh` |
| `0x1CDC` | `setipnum` |
| `0x1EDC` | `setienum` |
| `0x1FDC` | `clrienum` |
| `0x3004 + 4*(i-1)` | `target[i]` |

`domaincfg`：

```text
bit 8  IE
bit 2  DM   (1 = MSI)
bit 0  BE   (0 = little-endian)
```

`sourcecfg`：

```text
6 = Level1
7 = Level0
4 = Edge1
5 = Edge0
```

`target` MSI 模式：

```text
bits 31:18 Hart Index
bits 17:12 Guest Index
bits 10:0  EIID
```

## 7. 预计里程碑

| 里程碑 | 内容 | 预计工作量 |
|---|---|---|
| M1 | 7A 设备树解析与打印 | 0.5 个工作单元 |
| M2 | 7B 中断抽象，PLIC 回归 | 0.5–1 个工作单元 |
| M3 | 7C QEMU AIA 与 UART 中断 | 1–2 个工作单元 |
| M4 | 7D K3 IMSIC/APLIC | 1–2 个工作单元 |
| M5 | 7E UART 中断与 7F 回归文档 | 0.5–1 个工作单元 |

总计：4–6 个工作单元。

## 8. 风险与回退

| 风险 | 影响 | 回退 |
|---|---|---|
| K3 `smsiaddrcfg` 不可写或已锁定 | APLIC 无法生成 MSI | 使用固件配置；打印寄存器；必要时改用 direct mode（若 IMSIC 支持） |
| IMSIC hart 文件地址错误 | 中断不投递或写错页 | 按公式逐 hart 打印地址，单核先验证 |
| APLIC 源 42 模式错误 | UART 中断不触发 | 读取 `sourcecfg[42]`，尝试 Level1/Level0 |
| UART 中断风暴 | CPU 被中断占满 | 处理 IIR，清空 FIFO，必要时临时屏蔽 |
| 中断上下文与调度器竞态 | panic 或丢唤醒 | 先用单核，再 4 核、8 核 |
| QEMU AIA 与 K3 布局差异 | 原型通过但板卡失败 | 全部地址与属性从 DT 读取，不硬编码 |

## 9. 需要确认的决策

1. 是否按本计划执行“最小闭环”，暂不做完整设备树探测？
2. UART 中断是否只绑定 `boot_hart`？
3. 是否保留 `PollingUart` 作为回退路径？
4. 是否接受阶段 7 分成 7A–7F 多次提交？

默认建议：

```text
最小闭环
UART 中断绑定 boot_hart
保留轮询回退
按 7A–7F 分阶段提交
```

## 10. 参考资料

- RISC-V AIA v1.0：
  - IMSIC：<https://docs.riscv.org/reference/aia/v1.0/IMSIC.html>
  - APLIC：<https://docs.riscv.org/reference/aia/v1.0/AdvPLIC.html>
- QEMU RISC-V AIA：
  - <https://www.qemu.org/docs/master/specs/riscv-aia.html>
- Linux RISC-V AIA 驱动：
  - `drivers/irqchip/irq-riscv-imsic*.c`
  - `drivers/irqchip/irq-riscv-aplic*.c`
- 本仓库：
  - `docs/agents/alien/09-more-cores.md`
  - `docs/agents/alien/02-k3-notes.md`
  - `docs/agents/02-uboot-cheatsheet.md`

## 11. 附：QEMU AIA 设备树关键片段

```text
interrupt-controller@d000000 {
    compatible = "qemu,aplic", "riscv,aplic";
    reg = <0x0 0xd000000 0x0 0x8000>;
    msi-parent = <&imsic_s>;
    riscv,num-sources = <0x60>;
};

interrupt-controller@28000000 {
    compatible = "qemu,imsics", "riscv,imsics";
    reg = <0x0 0x28000000 0x0 0x2000>;
    interrupts-extended = <&cpu0_intc 9>, <&cpu1_intc 9>;
    riscv,num-ids = <0xff>;
};

serial@10000000 {
    interrupts = <0x0a 0x04>;
    interrupt-parent = <&aplic_s>;
};
```

## 12. 附：K3 设备树关键片段

```text
simsic: interrupt-controller@e0400000 {
    compatible = "spacemit,k3-imsics", "riscv,imsics";
    reg = <0x0 0xe0400000 0x0 0x400000>;
    interrupts-extended = <&cpu0_intc 9>, ... <&cpu7_intc 9>;
    riscv,guest-index-bits = <6>;
    riscv,hart-index-bits = <4>;
    riscv,num-guest-ids = <63>;
    riscv,num-ids = <511>;
};

saplic: interrupt-controller@e0804000 {
    compatible = "spacemit,k3-aplic", "riscv,aplic";
    reg = <0x0 0xe0804000 0x0 0x4000>;
    msi-parent = <&simsic>;
    riscv,num-sources = <512>;
};

uart0: serial@d4017000 {
    compatible = "spacemit,k3-uart", "intel,xscale-uart";
    reg = <0x0 0xd4017000 0x0 0x100>;
    reg-shift = <2>;
    interrupts = <42 IRQ_TYPE_LEVEL_HIGH>;
};
```

## 13. 实施前检查清单

- [ ] 计划审阅通过。
- [ ] QEMU AIA 运行参数确认。
- [ ] K3 板卡凭据有效。
- [ ] 记录 K3 `smsiaddrcfg` 与 `smsiaddrcfgh` 的读回值。
- [ ] 记录 QEMU 默认 PLIC 基线。
- [ ] 保留 `PollingUart` 回退路径。
- [ ] 确认 UART 中断绑定的 hart。
