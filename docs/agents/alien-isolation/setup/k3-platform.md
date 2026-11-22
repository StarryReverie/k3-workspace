# K3 平台

适用硬件：BananaPi BPI-SM10（SpacemiT K3）
适用系统：AlienOS `isolation`，提交见 [`../README.md`](../README.md)
适用工具：`nightly-2024-10-25`、QEMU 9 及以上、`mkimage`
记录时间：2026-11-22
相关文档：[`../architecture/README.md`](../architecture/README.md)、[`../testing/k3-qemu.md`](../testing/k3-qemu.md)

本文记录隔离内核的 K3 平台配置、诊断镜像与高地址 QEMU 回归。诊断镜像只初始化
可信基，不启动域；AIA 中断、XScale UART、SDHCI 与 PCIe 留给后续阶段。

## 平台配置

新增 `cfg(k3)`，与 `qemu_riscv`、`vf2` 并列。构建时设置 `PLATFORM=k3`。

| 项目 | 值 | 说明 |
|---|---|---|
| 链接地址 | `0x102200000` | `kernel/build.rs` 按平台选择 |
| 内存范围 | `0x102000000..0x300000000` | 来自 U-Boot 控制设备树 |
| 时间基准 | 24 MHz | `base/platform` 与 `domain-lib/config` 均为 24 MHz |
| 控制台 | SBI | 与 QEMU 基线的内核输出路径一致 |
| 定时器 | SBI TIME | 诊断模式不开启中断 |
| 关机 | SBI SRST | — |
| 设备地址空间 | `0xd4017000..0xd4018000` | 只映射 UART，供 M3 使用 |

平台模块位于 `base/platform/src/k3_riscv/`，与 QEMU 平台相同，从启动时传入的
DTB 构造 `MachineInfo`。K3 的 DTB 没有 PLIC 节点，`plic` 字段保持 `0..0`；
`clint` 字段解析到 `0xf1810000..0xf1820000`，不用于外部中断。

## 诊断模式

K3 没有 PLIC 与 virtio，UART 也不是 16550 兼容模型。现有域装载要求：

- 平台设备中存在 `plic`；
- `vfs` 域依赖 `net_stack`；
- `net_stack` 依赖 `nic`；
- init 域集合包含 `uart16550` 与 `virtio_mmio_block`。

因此 M2 增加内核特性 `k3-diag`。该模式在完成以下步骤后输出 `K3_BOOT_OK`
并空闲：

```text
DTB 解析
MachineInfo 打印
帧分配器初始化
initrd 重定位
内核页表建立
分页开启
从 hart 启动
```

不执行 `bus::init_with_dtb`、`trap::init_trap_subsystem` 与
`domain::load_domains`。这样可以在没有 AIA 与 K3 UART 域的情况下验证 TCB。

`cfg(k3)` 在 `kernel/src/domain/mod.rs` 中为 `PlicType` 选择了 SiFive 分支，
只是为了通过类型检查；诊断模式不会走到该代码。

## Shell 模式

M3 增加 `--mode shell`。该模式构建 K3 域、用户 init 与 bash，并把 initramfs
嵌入内核，启动后进入轮询 UART shell。控制台细节见
[`k3-console.md`](k3-console.md)，测试见 [`../testing/k3-shell.md`](../testing/k3-shell.md)。

```bash
./scripts/alien-isolation/build-k3.sh --mode shell --smp 2
./scripts/alien-isolation/test-k3-shell.py --build all
```

诊断模式仍保留，用于只验证 TCB 的场景。

## 构建

```bash
nix develop
./scripts/alien-isolation/build-k3.sh --smp 8
```

脚本执行：

1. 以 `PLATFORM=k3`、`SMP=8`、`KERNEL_BASE=0x102200000` 构建内核，
   启用 `k3-diag`。
2. 生成只含 `tests/init` 占位文件的 initramfs。
3. 用 `llvm-objcopy` 生成 raw binary，gzip 压缩。
4. 生成 FIT，load 与 entry 均为 `0x102200000`。
5. 检查 ELF 入口地址。

产物位于 `alien-isolation/build/k3/smp-8/`：

```text
kernel-k3              ELF，162 KiB
kernel-k3.bin.gz       gzip 后的 raw binary
initramfs.cpio.gz      诊断用 initrd，约 155 B
alien-k3-diag.itb      U-Boot FIT，82 KiB
k3.its                 FIT 源文件
build-info             平台、模式、核数、基址与入口
```

FIT 只包含内核。诊断模式不需要用户态，因此不打包 ramdisk。

## 高地址 QEMU

```bash
./scripts/alien-isolation/run-k3-qemu.sh --smp 8
```

默认参数：

```text
-M virt -bios default
-m 10G
-smp 8
-kernel kernel-k3
-initrd initramfs.cpio.gz
```

QEMU 的 RAM 从 `0x80000000` 开始，10 GiB 覆盖到 `0x300000000`，能够容纳
K3 的链接地址与内存上界。该回归验证高地址、页表与帧分配范围，不验证 K3
固件与 UART。

验收输出：

```text
This is a device tree representation of a riscv-virtio,qemu machine
SMP:    8
Memory: 0x80000000..0x300000000
PLIC:   0x0..0x0
CLINT:  0x2000000..0x2010000
Initrd: Some(0x122200000..0x1225f64d4)
Relocate initrd data to 0x1062e9000
Frame allocator init success
Build kernel address space success
Activate paging mode success
K3_BOOT_OK
hart 0 start...
...
```

场景与断言见 [`../testing/k3-qemu.md`](../testing/k3-qemu.md)。

## 上板

`build-k3.sh` 生成 FIT 后，通过 `scripts/alien/boot-k3-board.py` 上传并启动：

```bash
./scripts/alien/boot-k3-board.py \
  --image alien-isolation/build/k3/smp-8/alien-k3-diag.itb \
  --marker K3_BOOT_OK
```

脚本连接 `ostool` 板卡会话，上传到 `boot/`，初始化 BPI-SM10 的 DHCP 与 PHY，
再执行：

```text
tftpboot 0x140000000 alien-k3-diag.itb
bootm 0x140000000 - $fdtcontroladdr
```

FIT 不含设备树，`bootm` 必须显式传入 U-Boot 控制设备树地址；省略第三个参数
会报 `Device tree not found or missing FDT support`。

板端 `MachineInfo` 与预期一致：

```text
This is a device tree representation of a spacemit k3 com260 ifx board machine
SMP:    16
Memory: 0x102000000..0x300000000
PLIC:   0x0..0x0
CLINT:  0xf1810000..0xf1820000
Initrd: None
Bootargs: Some(" boot_mode=nor")
```

DTB 报告 16 个 hart，诊断内核只启动 `CPU_NUM` 范围内的前 N 个。1/2/8 核均在
真实板卡上输出 `K3_BOOT_OK`，8 核时 7 个从 hart 全部启动。详细结果见
[`../testing/k3-board.md`](../testing/k3-board.md)。

## 上板问题

- U-Boot 的 DHCP 耗时可能超过固定等待，过早发送 `tftpboot` 会丢失命令，随后
  `bootm` 报 `Wrong Image Format`。启动脚本已改为等待 `DHCP client bound`。
- `ostool` 刷新令牌与上传线程存在短时竞态，偶发 `TOK is not set`。重新运行
  `ostool auth status` 后重试即可。
- 网卡初始化偶发 `EQOS_DMA_MODE_SWR stuck`，会话随后被服务端回收。更换板卡
  会话后重试成功。
- 串口输入偶发字符损坏，例如 `netretry` 显示为 `netzetry`。命令成功执行，
  但日志中的命令原文不可作为逐字依据。

## 已知限制

- 诊断模式不启动域；shell 模式已提供用户态与轮询控制台。
- 未初始化 IMSIC、APLIC 或 PLIC，外部中断不可用。
- 内核日志仍走 SBI，用户态控制台走 UART MMIO。
- 域构建必须使用 `PLATFORM=k3`，否则会退回 QEMU 配置。
- FIT 只包含内核；shell 模式的 initramfs 嵌入内核。
