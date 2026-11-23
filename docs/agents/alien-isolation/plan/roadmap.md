# 粗略路线

日期：2026-11-22
状态：规划
范围：从源码导入到 K3 隔离域、存储与 PCIe/NVMe 的阶段性路线

## 总体判断

旧 `alien/` 是宏内核结构，`alien-isolation/` 是“可信基 + 可动态装载隔离域”
结构。两者不能按目录直接合并；可复用的是平台事实、AIA 与 UART 细节、
SDHCI/NVMe 协议逻辑，以及测试与上板工具的思路。

新树中，平台启动、内存、trap、任务与域加载属于 TCB；中断控制器、UART、
块设备、文件系统、调度器等作为域运行。驱动域禁止 `unsafe`，因此旧驱动
不能原样复制。

## 阶段

| 阶段 | 目标 | 主要工作 | 验收 |
|---|---|---|---|
| M0 基线与构建 | 跑通未修改的 isolation | 非交互 QEMU 构建脚本；kernel、domains、initramfs、shell | QEMU 启动到 shell，域正常装载 |
| M1 回归骨架 | 固化可重复测试 | 复用 PTY 会话库；1/2/4/8 核、磁盘、动态域加载场景 | 场景可重复执行，日志稳定 |
| M2 K3 可信基 | 让 K3 进入内核与最小域集合 | 新增 `k3` 平台配置；链接地址、内存与设备地址空间；DTB、SBI、CPU 数量；高地址 QEMU 回归 | K3 诊断镜像启动；1/2/8 核可用 |
| M3 中断与控制台 | 替换 PLIC 假设 | AIA 中断域；K3 UART 的 XScale 适配；控制台输入与 IRQ 传递 | K3 上可交互 shell，多核正常 |
| M4 K3 存储 | 块设备与文件系统域 | SDHCI 只读块设备域；FAT 域挂载 `/mnt`；QEMU virtio 不回归 | 板上列出并读取文件 |
| M5 PCIe/NVMe | 在域架构下接入 NVMe | TCB 内 QEMU ECAM 与 K3 主机桥；APMU、PHY、PERST#、LTSSM、iATU；NVMe 块设备域；分区与 FAT | QEMU 与 K3 均能 Identify、读取文件 |
| M6 收尾 | 固化成果 | 干净构建；完整测试矩阵；文档；分组提交；可选域故障与重载测试 | 全部场景通过，工作区干净 |

M0 已完成，记录见 [`../setup/qemu-baseline.md`](../setup/qemu-baseline.md)。M1 已完成，
记录见 [`../testing/qemu-matrix.md`](../testing/qemu-matrix.md)：QEMU 可按 1/2/4/8 核
重复构建与启动，`dvfs` 可从 FAT 盘装载 `vfs2` 并替换 `vfs` 域，日志降噪后可稳定断言。

M2 已完成，记录见 [`../setup/k3-platform.md`](../setup/k3-platform.md)、
[`../testing/k3-qemu.md`](../testing/k3-qemu.md) 与
[`../testing/k3-board.md`](../testing/k3-board.md)：K3 内核链接到 `0x102200000`，
在高地址 QEMU 与真实 BananaPi BPI-SM10 上以 1/2/8 核完成 DTB、内存、分页、
initrd 重定位与从 hart 启动，FIT 可由 U-Boot `bootm` 加载。

M3 已完成，记录见 [`../setup/k3-console.md`](../setup/k3-console.md)、
[`../setup/k3-aia.md`](../setup/k3-aia.md) 与
[`../testing/k3-shell.md`](../testing/k3-shell.md)：域构建切换到 `PLATFORM=k3`，
K3 XScale UART 由 IMSIC/APLIC 驱动，QEMU 使用 UART IRQ 10，板端使用 IRQ 42。
QEMU 1/2/8 核与真实板卡 2/8 核均可交互。

M4 已完成，记录见 [`../setup/k3-storage.md`](../setup/k3-storage.md) 与
[`../testing/k3-storage.md`](../testing/k3-storage.md)：新增 `k3_sd` 只读块设备
域，复用 `shadow_blk`/`cache_blk` 块栈，在板端把 SD 卡 FAT 分区挂载到
`/mnt`，2 核与 8 核均能读取 `HELLO.TXT`。无卡时跳过块栈并继续启动。

M5A 已完成，记录见 [`../setup/qemu-nvme.md`](../setup/qemu-nvme.md) 与
[`../testing/qemu-nvme.md`](../testing/qemu-nvme.md)：TCB 增加 ECAM 枚举与
BAR 分配，新增只读 `nvme` 块设备域，QEMU 1/2/8 核均能 Identify、挂载 FAT
并读取文件。M5B 只读取证完成，但真实 K3 的两个已启用端口均 Link down，
U-Boot 枚举不到 NVMe，记录见
[`../setup/k3-pcie-recon.md`](../setup/k3-pcie-recon.md)。

## 与旧计划的对应

```text
旧阶段 0        -> M0
旧阶段 1–3      -> M2
旧阶段 4–7      -> M3
旧阶段 8        -> M4
旧阶段 9        -> M5
旧阶段 10       -> M6
```

## 主要风险

- isolation 只针对 QEMU 与 VF2，K3 需要同时修改 TCB、平台配置和域构建体系。
- PLIC 到 AIA 是架构替换，不是地址替换。
- QEMU 侧目前只有 virtio-mmio，NVMe 需要先补 PCIe 基线。
- 块设备与 NVMe 域需要安全的 MMIO 与 DMA 抽象，不能直接复用旧驱动。
- initramfs 要携带多个域 ELF，镜像体积与启动时间会上升。
- 第三方 crate 不 vendor，首次构建依赖网络或本地 Cargo 缓存。

## 下一步

先确认 K3 的 M.2 盘安装、供电与 PERST#，然后继续 M5B/M5C：

1. 重跑只读取证，直到 U-Boot `pci enum` 出现 `Link up` 与 `nvme info`。
2. 通过 iATU 访问配置空间，确认 `01:00.0 class=0x010802`。
3. 映射 BAR 与入站 DMA 窗口，把 BAR 区间接入现有 `nvme` 域。
4. K3 上完成 Identify、分区与 `/mnt` 挂载。
5. 保留 SDHCI 与 virtio 回退，补 QEMU 1/2/8 核与板端回归。
