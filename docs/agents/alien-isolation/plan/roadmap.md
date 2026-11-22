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

当前进入 M2：为 K3 增加平台配置、链接地址、内存与设备地址空间、DTB 与 SBI 支持，
并在高地址 QEMU 上回归。

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

先完成 M0：建立 `scripts/alien-isolation/`，构建 kernel、domains 与
initramfs，在 QEMU 上启动到 shell，并记录域装载日志。
