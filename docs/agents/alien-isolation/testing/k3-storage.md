# K3 存储回归

适用硬件：BananaPi BPI-SM10、QEMU `virt`
适用系统：AlienOS `isolation` 的 K3 shell 镜像
适用工具：`nightly-2024-10-25`、U-Boot 2022.10、`ostool`
记录时间：2026-11-22
相关文档：[`../setup/k3-storage.md`](../setup/k3-storage.md)、[`k3-shell.md`](k3-shell.md)

本文记录 SDHCI 块设备、块栈与 FAT 挂载的回归。板端只执行只读操作。

## 板端场景

| 场景 | 核数 | 验证内容 | 结果 |
|---|---|---|---|
| `board-storage-2` | 2 | SDHCI 识别、MBR、FAT、读取 `HELLO.TXT` | 通过 |
| `board-storage-8` | 8 | 全部 hart、SDHCI、FAT、读取 `HELLO.TXT` | 通过 |

上板命令：

```bash
./scripts/alien/boot-k3-board.py \
  --image alien-isolation/build/k3-shell/smp-2/alien-k3-shell.itb \
  --marker "Alien:/#" \
  --post-command 'echo M4_START' \
  --post-command 'echo M4_GLOB: /mnt/*' \
  --post-command 'mapfile -t lines < /mnt/HELLO.TXT' \
  --post-command 'echo M4_FIRST:${lines[0]}' \
  --post-command 'echo M4_END' \
  --post-wait 12 \
  --boot-timeout 150 \
  --timeout 700
```

8 核镜像将路径改为 `smp-8`，并延长 `--boot-timeout` 与 `--timeout`。

使用 `mapfile` 而不是 `$(< file)`：命令替换会 fork 一个子 shell，在当前
shell 实现上可能长时间不返回；`mapfile` 是内建命令，读取路径更稳定。

## 板端结果

2 核：

```text
sdhci fallback: spacemit,k3-sdhci 0xd4280000-0xd4280200
SDHCI: base=0xd4280000 base_clock=200MHz state=0x1f70000 clock=0x0 power=0xf caps=0x35fc0080
SDHCI: card ready rca=0xaaaa v2=true high_capacity=true
SDHCI: partition lba=8192 blocks=62325760 (30432 MiB)
sdcard: 62325760 blocks
<attach domain>: block, it's name is block-1
<attach domain>: shadow_blk, it's name is shadow_blk-1
<attach domain>: cache_blk, it's name is cache_blk-1
fatfs mount success
M4_GLOB: /mnt/HELLO.TXT
M4_COUNT:1
M4_FIRST:alien k3 sd mount ok
M4_END
```

8 核：

```text
SDHCI: partition lba=8192 blocks=62325760 (30432 MiB)
sdcard: 62325760 blocks
7 个从 hart 启动
M4_GLOB: /mnt/HELLO.TXT
M4_FIRST:alien k3 sd mount ok
M4_END
```

## QEMU 无卡路径

K3 镜像在 QEMU `virt` 上没有 SDHCI 节点，因此：

```text
blk domain not found, skip /dev/sda
no block device, skip FAT mount
```

shell 仍可交互。`test-k3-shell.py --build all` 的 1/2/8 核全部通过。

## QEMU 基线

`test-qemu.py --build all`：

```text
PASS smp-1
PASS smp-2
PASS smp-4
PASS smp-8
PASS disk-load
PASS alien-isolation qemu: 5 scenarios
```

virtio-mmio 块设备、`/dev/sda` 与 `/tests` FAT 挂载未回归。

## 已知问题

- 板端只验证读取；写入被驱动拒绝。
- SD 卡当前分区 1 起于 8192 号扇区，FAT 分区含 `HELLO.TXT`。
- 无卡、未分区或未格式化时仅打印原因，不影响启动。
- 串口日志中 `SDHCI` 与 shell 回显可能交错。

日志节选见 [`../logs/k3-storage.txt`](../logs/k3-storage.txt)。
