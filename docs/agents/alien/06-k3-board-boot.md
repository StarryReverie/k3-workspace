# K3 首次上板启动

日期：2026-11-10
范围：在 BananaPi BPI-SM10 上启动 Alien 诊断镜像，确认内核、分页、initramfs 与用户态 init
不涉及：bash 交互、UART 输入驱动、PLIC/APLIC/IMSIC 初始化、SMP、磁盘挂载

## 1. 目标

阶段 2 只在 QEMU 上验证了 K3 平台代码。本阶段完成三件事：

1. 用高地址 QEMU 回归覆盖 `0x102200000` 与 8 GiB 内存范围。
2. 构建不启动 bash 的诊断镜像，避免 `0x01` 洪泛干扰首次启动。
3. 在真实开发板上通过 U-Boot `tftpboot` 与 `bootm` 启动 Alien。

## 2. 上板前：高地址 QEMU 回归

阶段 2 的 QEMU 变体运行在 `0x80200000`，没有覆盖 K3 的链接地址。新增回归：

```bash
K3_DIAG=1 ./scripts/alien/build-k3.sh
QEMU_TIMEOUT=45 ./scripts/alien/run-k3-high-qemu.sh
```

QEMU 使用 10 GiB 内存，RAM 范围为 `0x80000000..0x300000000`，能够覆盖 K3 的 `0x102200000..0x300000000`。内核使用真实 K3 ELF 与 `linker-k3.ld`。

验收输出：

```text
Memory: 0x102000000..0x300000000
[0] Frame allocator init success
[0] Build kernel address space success
[0] Activate paging mode success
Init process is running
K3_BOOT_OK
```

该回归证明了高于 4 GiB 的地址、8 GiB 页表映射与帧分配器范围在 QEMU 上可用。

## 3. 诊断镜像

`build-k3.sh` 增加 `K3_DIAG=1`：

| 项目 | 完整镜像 | 诊断镜像 |
|---|---|---|
| 输出目录 | `out-k3` | `out-k3-diag` |
| FIT 名称 | `alien-k3.itb` | `alien-k3-diag.itb` |
| `/tests/bash` | 包含 | 不包含 |
| `/tests/libc.so` | 包含 | 不包含 |
| `/tests/k3-diag` | 不包含 | 包含 |
| init 行为 | 启动 bash | 打印 `K3_BOOT_OK` 后循环 `yield` |

用户态 `init` 在检测到 `/tests/k3-diag` 后打印标记，不再进入 bash。完整镜像行为保持不变。

## 4. 硬件信息采集

在 U-Boot 中通过 `fdt` 命令读取控制设备树，采集结果如下。

| 项目 | 值 |
|---|---|
| 板型 | `spacemit k3 com260 ifx board` |
| U-Boot | 2022.10 |
| DRAM | 8 GiB |
| `timebase-frequency` | `0x016e3600`，即 24 MHz |
| 内存节点 | `0x100000000..0x300000000` |
| Linux 可见内存 | `0x102000000..0x2ffffffff` |
| 保留区 | `0x100000000..0x101ffffff` |
| UART | `ns16550`，基址 `0xd4017000`，长度 `0x100`，`reg-shift = 2`，`reg-io-width = 4`，`clock-frequency = 0x00e10000` |
| 控制台 | `stdout-path = "uart0:115200"` |
| APLIC | `0xf1800000`、`0xe0804000`，两者 `status = "disabled"` |
| IMSIC | `0xf1000000`、`0xe0400000` |
| `fdtcontroladdr` | `0x2fbe903c0`，每次启动可能变化 |

采集命令：

```text
printenv fdtcontroladdr
fdt addr $fdtcontroladdr
fdt print /cpus
fdt list /soc
fdt print /memory
fdt print /soc/uart@d4017000
fdt print /soc/interrupt-controller@f1800000
fdt print /soc/interrupt-controller@e0804000
fdt print /soc/interrupt-controller@f1000000
fdt print /soc/interrupt-controller@e0400000
```

结论：

- 时间基准确认为 24 MHz，当前 `CLOCK_FREQ` 不需要修改。
- K3 使用 IMSIC，不是 PLIC；两个 APLIC 节点均处于禁用状态。
- UART 为 `ns16550`，但节点没有 `interrupts` 属性，第一版仍使用 SBI 控制台。

## 5. 上板流程

### 5.1 上传镜像

```bash
source scripts/session-env.sh logs/session-<timestamp>.log
./scripts/upload.sh alien/out-k3-diag/alien-k3-diag.itb boot/alien-k3-diag.itb
```

上传接口返回 `tftp_url`，本次路径为：

```text
ostool/sessions/<SID>/boot/alien-k3-diag.itb
```

### 5.2 U-Boot 网络初始化

```text
test -n "$ethaddr" || setenv ethaddr $eth1addr
setexpr.l sm10_dline *0xd4282bf0
setexpr sm10_dline $sm10_dline "|" 0x10001
mw.l 0xd4282bf0 $sm10_dline 1
setenv netretry no && setenv autoload no && dhcp && setenv autoload yes
```

### 5.3 加载与启动

第一次尝试：

```text
tftpboot 0x140000000 ostool/sessions/<SID>/boot/alien-k3-diag.itb
bootm 0x140000000
```

`bootm` 失败：

```text
Loading Kernel Image
Device tree not found or missing FDT support
### ERROR ### Please RESET the board ###
```

原因是当前 `tools/k3.its` 没有 FDT 节点。第二次显式传入 U-Boot 控制设备树：

```text
bootm 0x140000000 - $fdtcontroladdr
```

启动成功。U-Boot 将控制 FDT 重定位到 `0x2fbd6f000`，Alien 只保存指针，不解析设备树。

## 6. 验收结果

日志节选见 `logs/2026-11-10-k3-board-boot.txt`。关键输出：

```text
Starting kernel ...

[0] Boot hart 0
[0] This is a device tree representation of a SpacemiT K3 machine
SMP:    1
Memory: 0x102000000..0x300000000
PLIC:   0x0..0x0
CLINT:  0x0..0x0
Initrd: Some(
    0x102342003..0x102406d5b,
)
Bootargs: None
[0] Page start:0x12248e,end:0x300000,count:0x1ddb72
[0] Frame allocator init success
[0] Relocate initrd data to 0x00000001224ca000
[0] Build kernel address space success
[0] Activate paging mode success
[0] Initrd populate success
[0] Init filesystem success
[0] Init task success
[0] Begin run task...
Init process is running
K3_BOOT_OK
```

验收结论：

- U-Boot 能通过 TFTP 加载 FIT，并跳转到 `0x102200000`。
- SBI DBCN 控制台在真实 K3 上可用。
- 8 GiB 内存范围与页表映射可用，帧分配器管理约 7.46 GiB。
- 内嵌 initramfs 可以解包到 `/tests/init`。
- 用户态 init 可以运行并循环让出 CPU。
- 日志中 `0x01` 控制字符数量为 0。

## 7. 关键发现

1. **FIT 必须显式传入 FDT**
   `bootm <addr>` 会在加载内核后检查设备树。当前 FIT 不含 FDT，必须使用：

   ```text
   bootm <addr> - $fdtcontroladdr
   ```

   后续可以把设备树节点加入 FIT，减少对 U-Boot 控制设备树的依赖。

2. **时间基准为 24 MHz**
   FDT 中 `timebase-frequency = <0x016e3600>`，与当前配置一致。

3. **中断控制器为 IMSIC**
   APLIC 节点存在但被禁用，不能按 PLIC 的方式接入外部中断。

4. **高地址路径可用**
   真实硬件与 10 GiB QEMU 回归都验证了 `0x102200000` 以上的地址。

## 8. 已知问题

- 阶段 4 已完成完整 bash 上板验证，`0x01` 洪泛由轮询 UART 修复，见 `07-uart-console.md`。
- 用户态 UART 输入为忙等轮询，尚未接入中断。
- 未初始化 APLIC 或 IMSIC，外部中断不可用。
- 未启用 SMP，只使用启动 hart。
- 未挂载磁盘，根文件系统只来自 initramfs。
- 未验证长时间运行的定时器与调度精度。
- 阶段 3 的高地址 QEMU 回归脚本基于 SBI 控制台；UART 接入后不再适用于该路径。

## 9. 复现命令

```bash
# 采集 U-Boot 与设备树信息
./scripts/uboot-probe.py

# 诊断镜像
K3_DIAG=1 ./scripts/alien/build-k3.sh

# UART 输入自测镜像
K3_DIAG=1 K3_UARTTEST=1 ./scripts/alien/build-k3.sh

# 上板
./scripts/alien/boot-k3-board.py
```

`boot-k3-board.py` 会连接板卡、打断自动启动、上传镜像、执行网络初始化、`tftpboot` 与 `bootm`，并等待指定标记。默认标记为 `K3_BOOT_OK`，可用 `--marker` 与 `--post-command` 指定其他场景。

完整 bash 镜像上板：

```bash
./scripts/alien/build-k3.sh
./scripts/alien/boot-k3-board.py \
  --image alien/out-k3/alien-k3.itb \
  --marker "Alien:/#" \
  --post-command "echo K3_OK" \
  --post-command "/bin/busybox ls /"
```
