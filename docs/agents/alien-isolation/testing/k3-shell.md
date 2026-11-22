# K3 shell 回归

适用硬件：QEMU `virt`（10 GiB）、BananaPi BPI-SM10
适用系统：AlienOS `isolation` 的 K3 中断驱动控制台镜像
适用工具：`nightly-2024-10-25`、U-Boot 2022.10、`ostool`
记录时间：2026-11-22
相关文档：[`../setup/k3-console.md`](../setup/k3-console.md)、[`k3-board.md`](k3-board.md)

本文记录 K3 完整域集合、内嵌 initramfs、AIA 中断与 shell 的回归。控制台由
UART IRQ 驱动；没有 AIA 节点时回退到轮询。

## QEMU 场景

| 场景 | 核数 | 验证内容 |
|---|---|---|
| `k3-shell-1` | 1 | 域装载、VFS、init、shell |
| `k3-shell-2` | 2 | 1 个从 hart，shell 往返 |
| `k3-shell-8` | 8 | 7 个从 hart，shell 往返 |
| `all` | — | 依次执行以上三个场景 |

```bash
nix develop
./scripts/alien-isolation/test-k3-shell.py --build all
```

测试等待 `exec args: ["sh`、`AIA: enabled irq N`、全部从 hart 的
`hart N start...`，然后反复发送 `echo __M3_K3_READY__` 确认中断驱动的输入
输出链路，要求出现 `AIA/PLIC: irq N delivered`，最后要求 `M3_K3_OK`
独占一行。

## QEMU 结果

2026-11-22 执行：

```text
PASS k3-shell-1
PASS k3-shell-2
PASS k3-shell-8
PASS alien-isolation k3 shell: 3 scenarios
```

| 场景 | 启动 hart | 从 hart | UART IRQ | 结果 |
|---|---|---|---|---|
| `k3-shell-1` | 0 | 无 | 10 | 通过 |
| `k3-shell-2` | 0 | 1 | 10 | 通过 |
| `k3-shell-8` | 不固定 | 其余 hart | 10 | 通过 |

串口交互：

```text
AIA: enabled irq 10 for hart 0, sourcecfg=0x6 target=0xa
AIA/PLIC: irq 10 delivered
Alien:/# echo __M3_K3_READY__
__M3_K3_READY__
Alien:/# echo M3_K3_OK
M3_K3_OK
```

QEMU 默认使用 `-M virt,aia=aplic-imsic`，选中 S-level IMSIC
`0x28000000` 与 APLIC `0x0d000000`。`run-k3-shell-qemu.sh --no-aia` 可验证
`PlicType::None` 的轮询回退。

## 板端结果

镜像大小约 4.4 MB，FIT 只包含内核，initramfs 已嵌入内核。

| 场景 | 核数 | 关键输出 | 结果 |
|---|---|---|---|
| `board-shell-2` | 2 | `AIA selected: imsic=0xe0400000 aplic=0xe0804000`、`enabled irq 42`、`delivered`、`M3_K3_OK` | 通过 |
| `board-shell-8` | 8 | 7 个从 hart、`IRQ 42` 投递、`M3_K3_OK` | 通过 |
| `board-shell-1` | 1 | — | 未单独验证 |

2 核串口：

```text
AIA node: interrupt-controller@e0400000 ... hart_bits=4 guest_bits=6
AIA selected: imsic=0xe0400000 aplic=0xe0804000
uart8250: IER=0x41 MCR=0xb LSR=0x0 use_irq=true
AIA: enabled irq 42 for hart 0, sourcecfg=0x6 target=0x2a
AIA/PLIC: irq 42 delivered
Alien:/# echo M3_K3_OK
M3_K3_OK
```

8 核运行时 8 个 hart 均启动，UART IRQ 42 正常投递，shell 返回
`M3_K3_OK`。板端控制 DTB 有两套 S-level AIA，内核选择带
`hart_bits/guest_bits` 且与 UART 相连的 `e0400000/e0804000`。

## 上板命令

```bash
./scripts/alien/boot-k3-board.py \
  --image alien-isolation/build/k3-shell/smp-2/alien-k3-shell.itb \
  --marker "Alien:/#" \
  --post-command "echo M3_K3_OK" \
  --post-wait 8 \
  --timeout 700
```

8 核镜像将路径改为 `smp-8`，并延长 `--boot-timeout` 与 `--timeout`。由于
`Init process is running` 在 8 核下不稳定，建议直接等待 `Alien:/#`。

## 已知问题

- 上传 4.4 MB 镜像通常需要 40–90 秒，受宿主机到 `ostool` 服务端速率影响。
- 服务端偶发 DNS、会话申请失败或 `EQOS_DMA_MODE_SWR stuck`，重试即可。
- 8 核 U-Boot 启动日志中，早期用户态输出可能丢失或被拆散。
- 内核日志走 SBI，用户态控制台走 UART，两者可能交错；`delivered` 日志可能插入
  到 shell 回显中间，导致首字符看起来被覆盖。
- 不同固件版本的 K3 AIA 地址不同，当前同时映射 `0xe0...` 与 `0xf1...`。

日志节选见 [`../logs/k3-shell.txt`](../logs/k3-shell.txt) 与
[`../logs/k3-aia.txt`](../logs/k3-aia.txt)。
