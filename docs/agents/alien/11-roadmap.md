# 阶段 8–10 路线：存储与 NVMe

日期：2026-11-15
状态：路线规划；阶段 8 已完成，阶段 9 详细循环计划见 `13-pcie-nvme.md`
前置提交：`ba39b1c docs: record stage 7 UART interrupt completion`

## 0. 文档范围

本文件记录阶段 7 之后的大方向与阶段划分，不包含具体实施细节。每个阶段实施时
另行编写详细计划与实施记录。

## 1. 大方向

阶段 0–7 解决的是"能启动、能交互、多核可用、外部中断可用"。下一步从"能运行"
转向"有持久存储"：

```text
阶段 8   存储基础：QEMU virtio-blk 与 FAT，K3 SD 卡只读
阶段 9   PCIe 与 NVMe：QEMU 协议层，K3 链路，K3 读写
阶段 10  平台完善：控制台输出统一、A100、亲和性等
```

选择存储作为下一阶段的理由：

- 当前根文件系统完全内嵌在 initramfs 中，镜像 4 MiB，无法持久保存数据。
- 块设备、分区与文件系统是后续网络、日志、应用加载的共同基础。
- 阶段 7 已提供中断与设备树基础，块设备可以按同一模式接入。

## 2. 硬件事实

### 2.1 存储介质

| 介质 | 位置 | 现状 | 规划 |
|---|---|---|---|
| SD 卡 | `sdh@d4280000`，32 GB，`mmcblk0` 含 `p1` | U-Boot 可识别，历史日志有 `mmc0: new UHS-I speed SDR104 SDHC card` | 阶段 8 目标 |
| UFS | `ufs@c0e00000`，Kingston 128 GB | 板卡启动介质，`bootlogo loaded from ufs_fs (0:2)` | 暂不作为目标 |
| NVMe | M.2 Key M x4，对应 PCIe Port A | 历史日志 `PCIE-0: Link down` | 阶段 9 目标 |

板卡规格：两个 M.2 Key M（PCIe Gen3 x4 与 x1）与一个 Key E；
Port C 上已有 RTL8852 无线网卡。

### 2.2 K3 PCIe

| 项目 | 数值 |
|---|---|
| 控制器 | DesignWare 型，兼容 `spacemit,k1-pcie`，主线驱动为 `pcie-spacemit-k1.c` |
| 数量 | 5 个控制器（Port A–E）与 6 个 PHY |
| Port A 寄存器 | dbi `0x80000000`、dbi2 `0x80100000`、ATU `0x80300000` |
| Port A 配置空间 | `0x11_0000_0000`，64 KiB |
| Port A 非预取内存 | `0x11_0011_0000`，约 2 GiB |
| Port A 预取内存 | `0x18_0000_0000`，4 GiB |
| PHY 节点 | `phy-pcie@81d00000`，`num-lanes`、`spacemit,phy-id` |
| 时钟与复位 | `syscon_apmu` 提供 dbi、mstr、slv 三路 |
| PERST# | 默认由 PMU 寄存器控制，可通过 `reset-gpios` 改为 GPIO |
| 通道分叉 | Port A 与 Port B 共享 PHY1，由 bifurcation GPIO 决定 |
| 中断 | IMSIC 只接受 32 位 MSI；硬件不支持 MSI-X |

结论：NVMe 的协议层可以完全用轮询实现，不受 MSI 限制。

## 3. 阶段 8：存储基础

### 3.1 目标

- 有一个可用的块设备，注册为 `/dev/sda`。
- 能挂载文件系统并读写文件。
- QEMU 上走通"用户态从磁盘加载"的完整路径。
- K3 上完成 SD 卡容量识别与分区只读挂载。

### 3.2 非目标

- UFS 驱动。
- 分区管理与格式化。
- DMA 与中断驱动的块设备。
- `pivot_root` 式的完整根文件系统切换。

### 3.3 工作包

| 工作包 | 内容 | 验收 |
|---|---|---|
| 8A | QEMU `k3_qemu` 接入 virtio-blk 与 FAT，脚本生成磁盘镜像 | `/tests` 列出镜像文件，`cat` 内容正确 |
| 8B | K3 只读取证：U-Boot `mmc`、分区表、设备树、APMU 时钟 | 记录控制器版本、容量、分区与文件系统类型 |
| 8C | K3 SDHCI 最小只读驱动，PIO 方式，默认或高速模式 | 读 0 号扇区成功，`ls /mnt` 列出分区内容 |
| 8D | 回归、文档与提交 | QEMU 有盘/无盘、K3 1/2/8 核均通过 |

### 3.4 关键设计

- 挂载点：QEMU 挂 `/tests`（与上游一致，`init` 来自磁盘）；
  K3 挂 `/mnt`，保留 initramfs 里的 `/tests/init`，避免遮蔽初始进程。
- 文件系统：先 FAT32；若 8B 发现 K3 分区为 ext4，则启用 `ext` 特性。
- 控制器：K3 为**标准 SDHCI**，与 `visionfive2_sd` 的 DesignWare MSHC 布局不同，
  不能直接复用。
- 只读优先：K3 侧先只读，避免改动用户卡内容。

### 3.5 工作量

约 3–4.5 个工作单元。

## 4. 阶段 9：PCIe 与 NVMe

### 4.1 目标

- K3 上能枚举 PCIe 设备，识别 NVMe 硬盘。
- 能读出容量与型号，挂载分区并读取文件。
- 不要求写入、多队列与中断驱动。

### 4.2 非目标

- MSI/MSI-X 中断驱动。
- 多队列与高并发。
- 热插拔与电源管理。
- 32 位 DMA 端点的 bounce buffer。

### 4.3 实施方式

阶段 9 拆为 9A-1 至 9A-5、9B-1 至 9B-5、9C-1 至 9C-3 共 13 个小循环。
每个循环按“计划、实施、验证、整理、提交”完成，独立提交，不跨循环堆积
改动。详细目标、改动文件、验收命令与提交信息见 `13-pcie-nvme.md`。

### 4.4 里程碑

| 里程碑 | 内容 | 通过条件 |
|---|---|---|
| 9A | QEMU `virt` 上的 PCIe 枚举与 NVMe 协议层 | QEMU 挂载 NVMe 上的 FAT，读取正常 |
| 9B | K3 链路：APMU 时钟与复位、PHY 上电、PERST#、LTSSM、iATU | 能自主训练链路并读取 NVMe 配置空间 |
| 9C | K3 上的 NVMe Identify、分区与挂载，以及回归收尾 | `/mnt` 列出文件并读取成功；QEMU 有/无 NVMe、K3 1/2/8 核均通过 |

9A 在 QEMU 上完成，原因是 QEMU `virt` 自带 PCIe 主机桥与 `-device nvme`，
可以先把 ECAM 访问、BAR 分配、NVMe 队列与 PRP 全部验证，K3 只替换主机桥
初始化。

9B 是风险最高的部分，单独作为里程碑：只要能读到 NVMe 的配置空间
（厂商 ID、类代码 `0x010802`）即算通过。

### 4.5 工作量

约 6–8.5 个工作单元，约为阶段 7 的 1.5–2 倍。分项估算与循环拆分见
`13-pcie-nvme.md` 第 9 节。

## 5. 阶段 10：平台完善（候选）

| 项目 | 说明 | 工作量 |
|---|---|---|
| 控制台输出统一 | 内核日志改走 UART 驱动，消除与用户态输出的字符交错 | 0.5 |
| A100（CPU8–15） | 查询 HSM 状态并尝试启用 | 1–2 |
| `sched_setaffinity` | 调度器强制亲和性，支持 `taskset -p` | 1 |
| SD 写支持与热插拔 | SDHCI 写入、插拔检测 | 1–2 |
| kallsyms | 生成并加载符号表，改善栈回溯 | 0.5 |

## 6. 顺序理由

```text
阶段 8 先把块设备、分区与文件系统这一层打通
阶段 9 只替换传输后端，协议层在 QEMU 上先行验证
阶段 10 处理影响全局的工程债与剩余硬件
```

若阶段 8 的 SDHCI 驱动受阻，可只保留 QEMU 部分，把 SD 驱动并入阶段 9 之前
单独处理，不阻塞 NVMe 的 QEMU 侧工作。

## 7. 风险与回退

| 风险 | 影响 | 回退 |
|---|---|---|
| SDHCI 时钟、供电或 PHY 由私有模块控制 | 卡初始化超时 | 8A 与 8B 仍可交付，驱动后移 |
| K3 分区文件系统未知 | 选错文件系统特性 | 8B 先取证，必要时同时启用 `fat` 与 `ext` |
| 挂载点遮蔽 `/tests/init` | 无法启动 | QEMU 与 K3 使用不同挂载点 |
| PCIe PHY 寄存器序列缺乏公开资料 | 链路训练失败 | 以主线 K1 PHY 驱动为参照，仍失败则停在 9A |
| 分叉 GPIO 判断错误 | Port A 只有 x2 或不通 | 先降速或降宽运行 |
| 无 MSI-X、MSI 仅 32 位 | 中断驱动不可用 | 全程轮询完成队列 |
| 上板 DHCP 偶发失败 | 测试中断 | 沿用上板脚本重试 |

## 8. 低成本前置验证

在投入阶段 9 之前，可用 U-Boot 命令确认硬件通路（只读，不改动磁盘）：

```text
pci
pci enum
nvme scan
nvme info
```

若能枚举出 NVMe 设备，说明 PHY 与链路在厂商代码下可用，9B 的难度显著下降。
若不能，需要先确认插槽、供电与分叉开关。

该步骤已纳入 `13-pcie-nvme.md` 的 9B-1，作为进入代码阶段前的闸门。

## 9. 参考资料

- AIA 与阶段 7 记录：`10-interrupts.md`
- K3 平台事实：`02-k3-notes.md`
- 上游 QEMU 磁盘做法：`scripts/alien/build-qemu.sh`、`scripts/alien/run-qemu.sh`
- SDHCI 规范；Linux `drivers/mmc/host/sdhci-of-k1.c`
- PCIe：Linux `drivers/pci/controller/dwc/pcie-designware*.c`、
  `pcie-spacemit-k1.c`、`spacemit_pcie_phy.c`
- NVMe：NVMe Base Specification 2.x
