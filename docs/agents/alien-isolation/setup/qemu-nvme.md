# QEMU PCIe 与 NVMe 块设备

适用硬件：QEMU `virt`（K3 地址布局）、真实 K3
适用系统：AlienOS `isolation` 的 K3 shell 镜像
适用工具：`nightly-2024-10-25`、QEMU 11.x、`mkfs.fat`/`mcopy`
记录时间：2026-11-23
相关文档：[`k3-pcie-recon.md`](k3-pcie-recon.md)、[`k3-storage.md`](k3-storage.md)、
[`../testing/qemu-nvme.md`](../testing/qemu-nvme.md)、[`../architecture/README.md`](../architecture/README.md)

本文记录 M5A：QEMU `virt` 上的 PCIe ECAM 枚举、BAR 分配与 NVMe 只读块设备域。

## 分层

PCIe 与 NVMe 按现有 isolation 结构拆分：

| 层 | 位置 | 职责 |
|---|---|---|
| TCB | `kernel/src/bus/pci` | ECAM 访问、扫描 bus 0、BAR 探测与分配、打开 Memory/Bus Master |
| 域 | `domains/drivers/nvme` | NVMe 控制器复位、队列、Identify、PRD/PRP 读取、分区解析 |
| 块栈 | 现有域 | `block-1` → `shadow_blk-1` → `cache_blk-1` → `/dev/sda` → FAT |

TCB 只把 NVMe 的 BAR 物理区间传给域；域内仍禁止 `unsafe`，MMIO 走
`SafeIORegion`，DMA 页走 `FrameTracker`。

## QEMU 环境事实

QEMU `virt` 的主机桥兼容 `pci-host-ecam-generic`：

```text
ECAM       0x3000_0000，bus 0 占用 1 MiB
32 位 MMIO 0x4000_0000 起
NVMe       00:01.0，class 0x010802，BAR0 为 64 位、16 KiB
```

K3 平台配置同时用于板卡与高地址 QEMU 回归，因此在
`base/platform/src/k3_riscv/config.rs` 增加：

```text
PCI_BAR_WINDOW_START = 0x4000_0000
PCI_BAR_WINDOW_END   = 0x4010_0000
("pci-ecam-qemu", 0x3000_0000, 0x0010_0000)
("pci-bar-qemu",  0x4000_0000, 0x0010_0000)
```

真实 K3 使用 U-Boot 分配的 BAR 与 iATU，不走这条 ECAM 路径。

## 设备树探测

`probe_pci` 只匹配 compatible 为 `pci-host-ecam-generic` 的节点并取其第一个
`reg`。这样 K3 的 `spacemit,k3-pcie` 节点（第一个 `reg` 是 DBI，不是 ECAM）
不会被误判。QEMU 上日志为：

```text
pci: ECAM pci-host-ecam-generic 0x30000000-0x40000000
PCIe: ECAM base=0x30000000 size=0x10000000
```

## ECAM 枚举

配置地址：

```text
base + (bus << 20) + (dev << 15) + (func << 12) + (offset & 0xffc)
```

流程：

1. 扫描 bus 0 的 32 个设备。
2. 读 header type，多功能设备再扫 8 个 function。
3. 读 vendor/device、class、6 个 BAR。
4. BAR 大小探测：写 `0xffff_ffff`、读回掩码、恢复原值；支持 64 位 BAR。
5. 类代码 `0x010802` 视为 NVMe。

QEMU 结果：

```text
PCIe: 2 device(s), 1 NVMe
PCIe: 00:00.0 vendor=0x1b36 device=0x0008 class=0x060000 header=0x00
PCIe: 00:01.0 vendor=0x1b36 device=0x0010 class=0x010802 header=0x00
PCIe:   bar0 base=0x0 size=0x4000 mem64
```

## BAR 分配

无固件的 QEMU 不会预分配 BAR。TCB 在窗口内按 BAR 大小对齐分配，并写回：

```text
PCIe: NVMe BAR0 -> 0x40000000 size=0x4000, memory+bus master on
```

随后把 `0x40000000..0x40004000` 记录为 NVMe 设备区间。域创建时通过
`init_by_box` 传入该区间，域用 `SafeIORegion::from` 访问。

## NVMe 域

寄存器与队列：

```text
INTMS = 0xffff_ffff        屏蔽所有中断
CC.EN = 0                  停控制器，等待 CSTS.RDY = 0
ASQ/ACQ/AQA                管理队列，32 项，各一页
CC = EN | IOSQES=6 | IOCQES=4
Identify CNS=0x01          控制器型号/序列号/固件
Identify CNS=0x00          命名空间容量与 LBA 格式
Set Features FID=0x07      申请 1 个 I/O 队列
Create CQ 0x05 / SQ 0x01   各 32 项
```

门铃偏移为 `0x1000 + (2 * qid [+1]) * (4 << CAP.DSTRD)`。完成队列用 phase
位判断新条目，状态非零时打印 opcode 结果并返回错误。

读取使用 Read 命令（opcode `0x02`）：

- 不超过一页：只用 PRP1。
- 两页：PRP1 + PRP2。
- 更多页：一页 PRP 列表，PRP2 指向列表。

DMA 页来自 `FrameTracker::new(1)`，域内物理地址与虚拟地址恒等，可直接作为
PRP 地址，与 virtio 路径一致。

提交队列项用 8 字节 volatile 写，保证 opcode 与 CID（同在 dword 0）同时
可见；门铃前使用 `arch::fence_io()`。检查产物发现，自定义 target 上的
`core::sync::atomic::fence(SeqCst)` 只生成 `fence rw,w` / `fence r,rw`，
不覆盖设备 I/O，因此不能依赖它做 MMIO 屏障。

## 完成项异常与重试

QEMU TCG 上偶发出现“设备把有效命令的完成项写成 `CID=0`”。读取是幂等操作，
因此驱动在 CID 不符时重新提交同一条 Read（最多 3 次）；控制器初始化失败时
整体复位并重新初始化（最多 3 次）。压测 20 次读文件全部通过，其中一次在
Create SQ 阶段命中异常，复位重试后成功。

重试只在失败路径执行，正常路径没有额外开销。

## 分区与挂载

NVMe 域暴露第一个可挂载分区：

- 有保护性 MBR 时先读 GPT 项。
- 否则读 MBR 的 4 个分区项。
- 若 0 号扇区本身是 FAT 引导扇区，按整盘处理。

QEMU 测试镜像是整盘 FAT32，因此日志为：

```text
nvme: FAT partition lba=0 blocks=147456 (72 MiB)
nvme: 147456 blocks
```

块栈优先级为 NVMe > SDHCI > virtio。NVMe 成功后：

```text
<attach domain>: block, it's name is block-1
fatfs mount success
```

K3 平台的 FAT 挂载点是 `/mnt`，`/tests` 仍来自 initramfs。`write_block`
返回 `EROFS`，M5 保持只读。

## 构建与运行

```bash
./scripts/alien-isolation/build-k3.sh --mode shell --smp 2 --disk
./scripts/alien-isolation/run-k3-shell-qemu.sh --smp 2 --nvme
```

`--disk` 生成 72 MiB FAT32 镜像 `nvme.img`，内含 `HELLO.TXT`
（内容 `alien k3 nvme mount ok`）。`--nvme` 把它作为：

```text
-drive file=<out>/nvme.img,if=none,id=nvme0,format=raw
-device nvme,serial=deadbeef,drive=nvme0
```

不指定 `--nvme` 时镜像仍然可用，只是没有 PCIe NVMe 设备。

## 已知限制

- 只扫描 bus 0，不递归桥设备。
- 轮询，不使用 MSI/MSI-X，不注册 IMSIC 中断。
- 只读，`write_block` 返回 `EROFS`。
- 只支持 512 字节 LBA；4K LBA 时禁用块栈并打印原因。
- 只取第一个 NVMe 与第一个 FAT 分区，只创建一个 `/dev/sda`。
- 无热插拔、电源管理与多队列。
- QEMU 上偶发完成项 `CID=0`，通过幂等读重试与初始化复位重试恢复。
- 真实 K3 的 iATU、APMU、PHY 尚未接入链路，见
  [`k3-pcie.md`](k3-pcie.md) 与 [`k3-pcie-recon.md`](k3-pcie-recon.md)。
