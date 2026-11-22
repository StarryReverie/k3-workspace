# K3 shell 回归

适用硬件：QEMU `virt`（10 GiB）、BananaPi BPI-SM10
适用系统：AlienOS `isolation` 的 K3 轮询控制台镜像
适用工具：`nightly-2024-10-25`、U-Boot 2022.10、`ostool`
记录时间：2026-11-22
相关文档：[`../setup/k3-console.md`](../setup/k3-console.md)、[`k3-board.md`](k3-board.md)

本文记录 K3 完整域集合、内嵌 initramfs 与轮询 UART shell 的回归。镜像不启用
外部中断。

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

测试等待 `exec args: ["sh`、全部从 hart 的 `hart N start...`，然后反复发送
`echo __M3_K3_READY__` 确认输入输出链路，最后要求 `M3_K3_OK` 独占一行。

## QEMU 结果

2026-11-22 执行：

```text
PASS k3-shell-1
PASS k3-shell-2
PASS k3-shell-8
PASS alien-isolation k3 shell: 3 scenarios
```

| 场景 | 启动 hart | 从 hart | 日志大小 | 结果 |
|---|---|---|---|---|
| `k3-shell-1` | 0 | 无 | 27 KB | 通过 |
| `k3-shell-2` | 0 | 1 | 27 KB | 通过 |
| `k3-shell-8` | 7 | 0–6 | 29 KB | 通过 |

串口交互：

```text
Alien:/# echo __M3_K3_READY__
__M3_K3_READY__
Alien:/# echo M3_K3_OK
M3_K3_OK
```

## 板端结果

镜像大小约 4.4 MB，FIT 只包含内核，initramfs 已嵌入内核。

| 场景 | 核数 | 关键输出 | 结果 |
|---|---|---|---|
| `board-shell-2` | 2 | `uart fallback: ns16550 0xd4017000`、`Initrd: Some`、`M3_K3_OK` | 通过 |
| `board-shell-8` | 8 | 全部 hart、`Alien:/#` | 到提示符 |
| `board-shell-1` | 1 | — | 未单独验证 |

2 核串口：

```text
[0] uart fallback: ns16550 0xd4017000-0xd4017100
[0] register 2 devices
[1] hart 1 start...
Alien:/# echo M3_K3_OK
M3_K3_OK
```

8 核运行时 8 个 hart 均启动，日志末尾出现 `Alien:/#`。该次脚本以
`Init process is running` 为标记，8 核下该行可能丢失，因此没有发送后续命令；
交互输入由 2 核与 QEMU 8 核结果覆盖。

## 上板命令

```bash
./scripts/alien/boot-k3-board.py \
  --image alien-isolation/build/k3-shell/smp-2/alien-k3-shell.itb \
  --marker "Alien:/#" \
  --post-command "echo M3_K3_OK" \
  --post-wait 6 \
  --timeout 600
```

8 核镜像将路径与 `--smp` 改为 `8`。由于 `Init process is running` 在 8 核下
不稳定，建议直接等待 `Alien:/#`。

## 已知问题

- 上传 4.4 MB 镜像通常需要 40–90 秒，受宿主机到 `ostool` 服务端速率影响。
- 服务端偶发 DNS 或会话申请失败，重试即可。
- 8 核 U-Boot 启动日志中，早期用户态输出可能丢失或被拆散。
- 内核日志走 SBI，用户态控制台走 UART，两者可能交错。

日志节选见 [`../logs/k3-shell.txt`](../logs/k3-shell.txt)。
