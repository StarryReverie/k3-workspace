# K3 板端诊断启动

适用硬件：BananaPi BPI-SM10（SpacemiT K3）
适用系统：AlienOS `isolation` 的 K3 诊断内核
适用工具：`ostool`、U-Boot 2022.10、TFTP
记录时间：2026-11-22
相关文档：[`../setup/k3-platform.md`](../setup/k3-platform.md)、[`k3-qemu.md`](k3-qemu.md)

本文记录 K3 诊断镜像在真实开发板上的 FIT 加载、内存布局与 1/2/8 核启动。
诊断镜像不启动域，验证范围到可信基与从 hart 启动为止。

## 场景

| 场景 | 核数 | 验证内容 |
|---|---|---|
| `board-1` | 1 | FIT 加载、SBI 控制台、单 hart 初始化 |
| `board-2` | 2 | 1 个从 hart 启动 |
| `board-8` | 8 | 7 个从 hart 启动 |

镜像由 `build-k3.sh` 生成，诊断模式下 `CPU_NUM` 决定启动的 hart 数量。板端
DTB 报告 16 个 hart，内核只使用前 N 个。

## 流程

```bash
nix develop

./scripts/alien-isolation/build-k3.sh --smp 8
./scripts/alien/boot-k3-board.py \
  --image alien-isolation/build/k3/smp-8/alien-k3-diag.itb \
  --marker K3_BOOT_OK \
  --timeout 300
```

启动脚本执行：

1. 通过 `ostool` 申请板卡会话并中断 U-Boot 自动启动。
2. 上传 FIT 到当前会话的 `boot/` 目录。
3. 初始化 BPI-SM10 网卡，等待 DHCP 完成。
4. `tftpboot 0x140000000 <FIT>`。
5. `bootm 0x140000000 - $fdtcontroladdr`。
6. 等待 `K3_BOOT_OK`，保存串口记录到 `logs/boot-*.log`。

## 板端机器信息

```text
This is a device tree representation of a spacemit k3 com260 ifx board machine
SMP:    16
Memory: 0x102000000..0x300000000
PLIC:   0x0..0x0
CLINT:  0xf1810000..0xf1820000
Initrd: None
Bootargs: Some(" boot_mode=nor")
```

诊断 FIT 只包含内核，因此 `Initrd` 为 `None`。内存范围由 U-Boot 控制设备树
给出，已扣除 `0x100000000..0x101ffffff` 保留区。

## 验收结果

2026-11-22 在 BananaPi-1 与 BananaPi-2 上执行。

| 场景 | FIT 大小 | 启动 hart | 从 hart | 结果 |
|---|---|---|---|---|
| `board-1` | 83567 B | 0 | 无 | 通过 |
| `board-2` | 83987 B | 0 | 1 | 通过 |
| `board-8` | 85311 B | 0 | 1、2、3、4、5、6、7 | 通过 |

三个镜像均输出：

```text
[0] Activate paging mode success
[0] K3_BOOT_OK
```

8 核场景的从 hart 输出：

```text
[1] hart 1 start...
[2] hart 2 start...
[3] hart 3 start...
[4] hart 4 start...
[5] hart 5 start...
[6] hart 6 start...
[7] hart 7 start...
```

日志节选见 [`../logs/k3-board.txt`](../logs/k3-board.txt)。

## 上板问题

### DHCP 与 TFTP 时序

首次尝试在 DHCP 未完成时发送 `tftpboot`，命令丢失，随后 `bootm` 报
`Wrong Image Format for bootm command`。脚本已改为在串口出现
`DHCP client bound` 后再发送加载命令。

### 访问令牌

`ostool` 刷新令牌与上传线程存在短时竞态，偶发：

```text
error: TOK is not set
```

重新运行 `ostool auth status`，确认 `session-env.sh` 输出 `token : found`
后重试。

### 网卡初始化

一次 8 核尝试停在：

```text
EQOS_DMA_MODE_SWR stuckFAILED: -110
```

随后板卡会话被服务端释放。更换到另一块板卡后重试成功。该问题出现在 U-Boot
网络初始化阶段，与 Alien 内核无关。

### 串口字符损坏

U-Boot 回显偶发字符损坏，例如 `setenv netretry` 显示为 `setenv netzetry`。
命令仍然执行，但日志中的命令原文不能逐字引用。

## 已知限制

- 诊断模式不启动域，没有用户态与 shell。
- 未初始化 IMSIC、APLIC 或 PLIC。
- 内核输出走 SBI，未使用 K3 UART 驱动。
- FIT 只包含内核，不包含 ramdisk 与设备树。
