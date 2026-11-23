# QEMU NVMe 回归

适用硬件：QEMU `virt`、BananaPi BPI-SM10
适用系统：AlienOS `isolation` 的 K3 shell 镜像
适用工具：`nightly-2024-10-25`、QEMU 11.x、`ostool`
记录时间：2026-11-23
相关文档：[`../setup/qemu-nvme.md`](../setup/qemu-nvme.md)、
[`../setup/k3-pcie-recon.md`](../setup/k3-pcie-recon.md)、[`k3-shell.md`](k3-shell.md)

本文记录 M5A 的 PCIe/NVMe 回归。

## 场景

| 场景 | 核数 | NVMe | 验证内容 |
|---|---|---|---|
| `k3-nvme-1` | 1 | 有 | 枚举、Identify、FAT、读取 `HELLO.TXT` |
| `k3-nvme-2` | 2 | 有 | 同上，含从 hart 启动 |
| `k3-nvme-8` | 8 | 有 | 同上，8 个 hart |
| `k3-shell-1/2/8` | 1/2/8 | 无 | 无 PCIe 设备时回退，shell 正常 |

`all` 包含以上 6 个场景。

## 命令

```bash
./scripts/alien-isolation/test-k3-shell.py --build all
./scripts/alien-isolation/test-k3-shell.py --no-build k3-nvme-2
```

手工构建与运行：

```bash
./scripts/alien-isolation/build-k3.sh --mode shell --smp 2 --disk
./scripts/alien-isolation/run-k3-shell-qemu.sh --smp 2 --nvme
```

`--disk` 生成 72 MiB FAT32 镜像 `nvme.img` 与 `HELLO.TXT`；`--nvme` 以
`-device nvme,serial=deadbeef` 挂到 QEMU PCIe 总线。

## 结果

```text
PASS k3-shell-1
PASS k3-shell-2
PASS k3-shell-8
PASS k3-nvme-1
PASS k3-nvme-2
PASS k3-nvme-8
PASS alien-isolation k3 shell: 6 scenarios
```

NVMe 关键日志：

```text
PCIe: ECAM base=0x30000000 size=0x10000000
PCIe: 2 device(s), 1 NVMe
PCIe: 00:01.0 vendor=0x1b36 device=0x0010 class=0x010802 header=0x00
PCIe:   bar0 base=0x0 size=0x4000 mem64
PCIe: NVMe BAR0 -> 0x40000000 size=0x4000, memory+bus master on
nvme: bar 0x40000000-0x40004000
NVMe: controller ready, cap=0x4008200f0107ff
NVMe: I/O queue ready
NVMe: model=QEMU NVMe Ctrl serial=deadbeef fw=11.1.2
NVMe: namespace 1 blocks=147456 lba=512
nvme: FAT partition lba=0 blocks=147456 (72 MiB)
nvme: 147456 blocks
<attach domain>: block, it's name is block-1
fatfs mount success
M5_NVME:alien k3 nvme mount ok
```

无 NVMe 的 `k3-shell-*` 只枚举到主机桥：

```text
PCIe: 1 device(s), 0 NVMe
no block device, skip FAT mount
```

shell 正常，`/dev/sda` 与 FAT 均跳过。

## 非 K3 基线

`./scripts/alien-isolation/test-qemu.py --build all`：

```text
PASS smp-1
PASS smp-2
PASS smp-4
PASS smp-8
PASS disk-load
PASS alien-isolation qemu: 5 scenarios
```

virtio-mmio 块设备、`/dev/sda` 与 `/tests` FAT 未回归。

## 板端回退

K3 板端当前 PCIe 链路 Link down，没有 NVMe 设备，因此走 SD 卡回退：

```text
SDHCI: partition lba=8192 blocks=62325760 (30432 MiB)
sdcard: 62325760 blocks
<attach domain>: block, it's name is block-1
fatfs mount success
M5_FIRST:alien k3 sd mount ok
```

新增的 PCIe 探测在真实 K3 上没有输出，也没有改变启动路径。板端取证细节见
[`../setup/k3-pcie-recon.md`](../setup/k3-pcie-recon.md)。

## 已知限制

- NVMe 场景只验证读取；写入返回 `EROFS`。
- 只覆盖 QEMU 的整盘 FAT 镜像；分区镜像由 MBR/GPT 解析路径覆盖，但未单独
  建立自动化场景。
- 真实 K3 的 NVMe 路径等待硬件链路可用。
