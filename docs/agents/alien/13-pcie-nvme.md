# 阶段 9 计划：PCIe 与 NVMe

日期：2026-11-16
状态：9A 完成，等待 9B 只读取证
当前循环：9B-1，未开始
范围：QEMU `virt` 的 PCIe 枚举与 NVMe 协议层、K3 的 PCIe 链路与 NVMe 挂载
相关文档：`11-roadmap.md` 第 4 节、`12-storage.md`、`02-k3-notes.md`

## 0. 文档用途

本文档是阶段 9 的执行计划，把 9A、9B、9C 拆成若干个小循环。每个循环完成后，
在对应小节追加“实施结果”与日志链接；阶段结束时整理为实施记录。

路线级的方向、理由与风险仍在 `11-roadmap.md`；本文档只记录可执行的步骤、
验收条件与提交边界。

## 1. 目标与非目标

### 1.1 目标

- QEMU `virt` 上完成 PCIe 枚举、BAR 分配与 NVMe 协议层。
- K3 上能枚举 PCIe 设备，识别 M.2 NVMe 硬盘。
- 能读出型号、容量与 LBA 格式，解析分区并挂载 FAT，读取文件。
- 每个循环可独立编译、独立验证、独立提交。

### 1.2 非目标

- MSI/MSI-X 中断驱动；阶段 9 全程轮询。
- 多队列、高并发、热插拔与电源管理。
- K3 上的 NVMe 写入；先只读。
- 32 位 DMA 端点的 bounce buffer；要求 NVMe 支持 64 位 DMA。
- UFS 驱动与 T100 IOMMU 的完整驱动。
- 多块设备节点；阶段 9 只保证一个 `/dev/sda`。

## 2. 实施原则

### 2.1 固定循环

每个小循环按以下顺序完成：

```text
1. 循环计划   在本文档对应小节写下目标、改动范围与验收命令
2. 实施       只改本节需要的文件，不提前引入下一节代码
3. 验证       QEMU 或板卡，保存日志到 docs/agents/alien/logs/
4. 整理       更新文档与进度，git diff --check，确认 CPU_NUM=1
5. 提交       一个循环一个提交，使用 feat: 或 docs: 前缀
6. 汇报       结果、遗留问题、下一循环入口
```

### 2.2 循环清单

| 循环 | 内容 | 状态 |
|---|---|---|
| 9A-1 | ECAM 枚举 | 完成，2026-11-16 |
| 9A-2 | 管理队列与 Identify | 完成，2026-11-16 |
| 9A-3 | I/O 队列与读取 | 完成，2026-11-16 |
| 9A-4 | 分区与挂载 | 完成，2026-11-16 |
| 9A-5 | 脚本、回归与文档 | 完成，2026-11-16 |
| 9B-1 | K3 只读取证 | 未开始 |
| 9B-2 | 配置空间访问 | 未开始 |
| 9B-3 | 控制器初始化与链路训练 | 未开始 |
| 9B-4 | PHY 与分叉 | 未开始 |
| 9B-5 | BAR、iATU 与完整枚举 | 未开始 |
| 9C-1 | K3 NVMe Identify | 未开始 |
| 9C-2 | K3 分区与挂载 | 未开始 |
| 9C-3 | 回归与收尾 | 未开始 |

### 2.3 停止条件

- 同一编译错误出现两次以上，停下分析，不连续盲改。
- QEMU 回归失败，先修复再进入下一循环。
- 板卡连续两次启动失败，停止上板，检查硬件与脚本。
- 涉及写盘、改固件、改启动介质，先征得同意。
- 发现需要扩大改动范围时，先更新本文档，再继续。

### 2.4 检查点

| 检查点 | 条件 | 后续 |
|---|---|---|
| 9A 结束 | QEMU 挂载 NVMe 上的 FAT，回归通过 | 进入 9B 只读取证 |
| 9B 结束 | K3 能自主训练链路并读取 NVMe 配置空间 | 进入 9C |
| 9C 结束 | K3 挂载 NVMe 分区并读取文件，1/2/8 核通过 | 收尾提交 |

每个检查点停下汇报，不连续跨越。

### 2.5 构建与提交约定

- 提交前把 `alien/subsystems/config/src/lib.rs` 的 `CPU_NUM` 恢复为 `1`。
- 不把口令、令牌、私钥写入仓库。
- 板卡日志先落到 `logs/`，再节选到 `docs/agents/alien/logs/`。
- 文档与代码分开提交；一个循环最多两个提交（实现与文档）。

## 3. 9A：QEMU 协议层

### 3.0 环境事实

QEMU 11.1.2 的 `virt` 平台提供标准 PCIe 主机桥：

| 区域 | 地址 | 大小 |
|---|---|---|
| ECAM | `0x3000_0000` | 256 MiB，bus 0–255 |
| 32 位 MMIO | `0x4000_0000` | 1 GiB |
| 64 位 MMIO | `0x4_0000_0000` | 16 GiB |

`k3_qemu` 的 MMIO 列表需要补上：

```text
(0x3000_0000, 0x1000_0000)   ECAM
(0x4000_0000, 0x0010_0000)   BAR 窗口，1 MiB 足够
```

QEMU 侧使用 `-device nvme,drive=x0,serial=deadbeef`，不加载 virtio-blk。
测试镜像可复用 64 MiB FAT32 镜像；若镜像没有分区表，走整盘 FAT 回退。

### 3.1 9A-1：ECAM 枚举

**目标**

建立 PCIe 配置空间访问与设备扫描，不涉及 NVMe 协议。

**改动**

- 新增 `subsystems/drivers/src/pci.rs`：
  - `PciAddress`、`PciBar`、`PciHost` trait。
  - `EcamHost`：配置地址为
    `base + (bus << 20) + (dev << 15) + (func << 12) + (off & 0xffc)`。
  - 扫描 bus 0 的 32 个设备与 8 个功能，处理多功能设备。
  - 读取厂商 ID、设备 ID、头部类型、类代码与 BAR。
  - BAR 大小探测：写入 `0xffff_ffff`，读回掩码，恢复原值。
  - 支持 64 位 BAR，优先分配到 `0x4000_0000` 起的 32 位窗口。
- `subsystems/drivers/src/lib.rs` 注册 `pci` 模块。
- `subsystems/platform/src/k3_riscv/config.rs` 为 `k3_qemu` 增加 ECAM 与
  BAR 窗口。
- `subsystems/devices/src/lib.rs` 增加只读探测函数，在 `k3_qemu` 启动时调用。

**验收**

```text
PCIe: 00:01.0 vendor=0x... device=0x... class=0x010802
```

设备位置以枚举结果为准。无 NVMe 时打印“未发现设备”，系统继续启动。

**提交**

```text
feat: enumerate PCIe devices on qemu virt
```

**实施结果（2026-11-16）**

```text
PCIe: ECAM base=0x30000000
PCIe: 2 device(s), 1 NVMe
PCIe: 00:00.0 vendor=0x1b36 device=0x0008 class=0x060000 header=0x00
PCIe: 00:01.0 vendor=0x1b36 device=0x0010 class=0x010802 header=0x00
PCIe:   bar0 base=0x0 size=0x4000 mem64
```

- 无 NVMe 时输出 `1 device(s), 0 NVMe`，系统继续启动。
- 实现提交：`cdd1f08 feat: enumerate PCIe devices on qemu virt`。
- 日志：`docs/agents/alien/logs/2026-11-16-k3-qemu-pcie-enum.txt`。

### 3.2 9A-2：管理队列与 Identify

**目标**

完成 NVMe 控制器复位、管理队列与 Identify，打印型号与容量。

**改动**

- 新增 `subsystems/drivers/src/nvme.rs`：
  - 寄存器定义：`CAP`、`VS`、`CC`、`CSTS`、`AQA`、`ASQ`、`ACQ` 与门铃。
  - 复位：`CC.EN=0`，等待 `CSTS.RDY=0`。
  - 管理队列：32 项，`ASQ`、`ACQ` 各一页，物理地址来自 `alloc_frames`。
  - 启动：`CC = EN | CSS=0 | MPS=0 | IOSQES=6 | IOCQES=4`，等待 `RDY=1`。
  - Identify Controller：CNS=`0x01`。
  - Identify Namespace：CNS=`0x00` 或 `0x02`。
  - 打印型号、序列号、固件版本、`nsze`、`ncap`、`nuse`、`lbaf`。
- 队列内存清零，命令超时设为 1 秒；超时打印状态并返回错误。

**验收**

```text
NVMe: model=QEMU NVMe Ctrl serial=deadbeef fw=...
NVMe: namespace 1 blocks=... lba=512
```

型号与序列号应与 QEMU 参数一致。失败时打印 `CSTS` 与 `CAP`，不 panic。

**提交**

```text
feat: add NVMe admin queue and identify
```

**实施结果（2026-11-16）**

```text
NVMe: BAR0 base=0x40000000 size=0x4000
NVMe: controller ready, cap=0x4008200f0107ff
NVMe: model=QEMU NVMe Ctrl serial=deadbeef fw=11.1.2
NVMe: namespace 1 blocks=131072 lba=512
```

- 无 NVMe 时输出 `NVMe: no device`，系统继续启动。
- Identify 首次返回全零，原因是 PRP1 写在 dword 4–5；修正为 dword 6–7。
- 实现提交：`c744fea feat: add NVMe admin queue and identify`。
- 日志：`docs/agents/alien/logs/2026-11-16-k3-qemu-nvme-identify.txt`。

### 3.3 9A-3：I/O 队列与读取

**目标**

建立 I/O 队列，完成单块与多块读取，验证 PRP。

**改动**

- 先用 Set Features Number of Queues（FID=`0x07`）申请 1 个 I/O 队列。
- 创建 1 个 I/O CQ（opcode `0x05`）与 1 个 I/O SQ（opcode `0x01`），
  各 32 项，物理连续。
- 实现 doorbell：`0x1000 + queue_id * 2 * (4 << CAP.DSTRD)`。
- 实现 PRP：
  - 不超过一页：PRP1。
  - 跨两页：PRP1 + PRP2。
  - 超过两页：分配一页 PRP 列表，PRP2 指向列表；完成后释放。
- 读取 LBA0，打印前 16 字节与 510、511 字节。
- 先只实现读取；`write_block` 返回只读错误，写入留到后续阶段。

**验收**

- 整盘 FAT 镜像：LBA0 为 FAT 引导扇区，`510/511` 为 `55AA`。
- 分区镜像：LBA0 为 MBR，`510/511` 为 `55AA`。
- 打印前 16 字节用于诊断；连续读取同一扇区两次，内容一致。

**提交**

```text
feat: add NVMe I/O queue and block reads
```

**实施结果（2026-11-16）**

```text
NVMe: namespace 1 blocks=98304 lba=512
NVMe: LBA0 head=[eb, 58, 90, 6d] sig=55aa
NVMe: LBA0..7 head=[eb, 58, 90, 6d] sig=55aa
```

- 首次 I/O 读取超时，原因是 Create CQ/SQ 未设置 PRP1，QEMU 将队列
  建在地址 0；补上队列页地址后恢复。
- 单块与跨页多块读取均读到 FAT 引导签名 `55aa`。
- 实现提交：`85516c0 feat: add NVMe I/O queue and block reads`。
- 日志：`docs/agents/alien/logs/2026-11-16-k3-qemu-nvme-read.txt`。

### 3.4 9A-4：分区与挂载

**目标**

把 NVMe 接入现有块层，解析分区并挂载 FAT。

**改动**

- 新增 `subsystems/drivers/src/partition.rs`：
  - MBR：读取 0 号扇区，遍历 4 个分区项。
  - GPT：识别保护性 MBR（类型 `0xEE`），读取 LBA1 的 `EFI PART`
    与 LBA2 的分区项。
  - 对候选分区读取首扇区，检查 FAT 引导签名 `0x55AA`。
  - 没有分区表但 0 号扇区本身是 FAT：按整盘处理。
- 新增 `PartitionBlockDevice`：
  ```rust
  struct PartitionBlockDevice {
      inner: Box<dyn LowBlockDevice>,
      lba_offset: u64,
      blocks: usize,
  }
  ```
- 新增 `NvmeBlockDevice`，实现 `LowBlockDevice`；阶段 9 的 `write_block`
  返回只读错误，避免误写。
- `devices::init_k3_nvme()`：枚举、初始化、Identify、分区、注册
  `/dev/sda`。
- `kernel/src/lib.rs` 调整启动顺序：NVMe 先于 SD 与 virtio，保证 NVMe
  成为 `/dev/sda`；任一后端失败时继续尝试下一个。

**验收**

```text
mount fs success at /tests
Alien:/# /bin/busybox ls /tests
HELLO.TXT
Alien:/# /bin/busybox cat /tests/HELLO.TXT
alien k3 nvme mount ok
```

**提交**

```text
feat: mount FAT from NVMe on qemu
```

**实施结果（2026-11-16）**

```text
NVMe: FAT partition lba=0 blocks=147456
NVMe: registered /dev/sda
mount fs success at /tests

Alien:/# /bin/busybox ls /tests
HELLO.TXT  bash       init       libc.so
Alien:/# /bin/busybox cat /tests/HELLO.TXT
alien qemu nvme mount ok
```

- 整盘 FAT32 镜像被识别为 `lba=0` 的整盘分区。
- `NvmeBlockDevice` 的写入返回 `EROFS`，阶段 9 保持只读。
- 实现提交：`a7c94c2 feat: mount FAT from NVMe on qemu`。
- 日志：`docs/agents/alien/logs/2026-11-16-k3-qemu-nvme-mount.txt`。

### 3.5 9A-5：脚本、回归与文档

**目标**

把 QEMU NVMe 路径纳入脚本与回归，完成 9A 记录。

**改动**

- `scripts/alien/run-k3-qemu.sh` 增加 `QEMU_NVME=1`：
  有磁盘时选择 `nvme`，否则保持 `virtio-blk-device`。
- `scripts/alien/build-k3-qemu.sh` 在磁盘镜像中生成 `HELLO.TXT`。
- 保存 NVMe、virtio、无磁盘与 AIA 的回归日志。

**验收矩阵**

| 环境 | 结果 |
|---|---|
| QEMU NVMe + FAT | `mount fs success at /tests`，`ls` 与 `cat` 正常 |
| QEMU virtio-blk + FAT | `mount fs success at /tests`，`ls` 与 `cat` 正常 |
| QEMU 无磁盘、无 NVMe | `NVMe: no device`，shell 正常 |
| QEMU AIA + NVMe | 中断控制台正常，挂载与读取正常 |
| `QEMU_NVME=1` 无磁盘 | 退出码 2，打印参数错误 |

**实施结果（2026-11-16）**

```bash
K3_QEMU_DISK=1 ./scripts/alien/build-k3-qemu.sh
QEMU_DISK=1 QEMU_NVME=1 QEMU_TIMEOUT=30 ./scripts/alien/run-k3-qemu.sh
```

- 实现提交：`7f963eb feat: add QEMU NVMe regression mode`。
- 回归已固化为 `scripts/alien/test-qemu.py`，场景与用法见
  `14-testing.md`。
- 日志：`docs/agents/alien/logs/2026-11-16-k3-qemu-nvme-regression.txt`。

9A 到此完成，进入 9B-1 只读取证。

## 4. 9B：K3 链路

### 4.1 9B-1：只读取证

**目标**

确认硬件、固件与设备树状态，决定是否进入代码阶段。

**步骤**

```text
pci
pci enum
nvme scan
nvme info
fdt print /soc/pcie@80000000
fdt print /soc/phy@81d00000
```

同时读取：

- 从设备树 `spacemit,apmu` 找到 APMU 基址，读取 `+0x1f0` 与 `+0x1f4`。
- link 寄存器 `0x8290_0000`，读取 `+0x0` 与 `+0x4`。
- PHY 寄存器 `0x81d0_0000`，读取前 32 个字。
- T100 IOMMU 的 `status`、控制寄存器与 `dma-ranges`。
- 分叉 GPIO、`reset-gpios`、`num-lanes` 与 `spacemit,phy-id`。

**记录内容**

- NVMe 型号、容量、固件版本。
- 链路速率与宽度，或 `Link down` 的具体状态。
- U-Boot 留下的 BAR 分配与 iATU 视口。
- APMU 时钟、复位、PERST# 与 LTSSM 状态。
- IOMMU 是否启用、是否旁路。

**验收**

形成一页事实记录，明确“U-Boot 能枚举”或“U-Boot 不能枚举”。
若不能枚举，停止代码工作，先检查插槽、供电、分叉与 PERST#。

**提交**

```text
docs: record stage 9B pcie recon
```

### 4.2 9B-2：配置空间访问

**目标**

在不改动 U-Boot 初始化结果的前提下，通过 iATU 读取 NVMe 配置空间。

**改动**

- 新增 `subsystems/drivers/src/pci_spacemit.rs`：
  - `SpacemitHost`，持有 DBI、ATU、配置窗口与 link 基址。
  - `config_read`/`config_write`：每次访问重写 ATU Outbound 0 的目标地址。
  - 配置类型 `CFG0`，目标地址为
    `(bus << 20) | (dev << 15) | (func << 12) | (off & !3)`。
- `subsystems/platform/src/k3_riscv/config.rs` 增加 Port A 的 MMIO 映射：
  ```text
  (0x8000_0000, 0x0040_0000)   DBI、DBI2、ATU
  (0x8290_0000, 0x0000_1000)   link
  (0x81d0_0000, 0x0000_1000)   PHY，大小以设备树为准
  (0x11_0000_0000, 0x0001_0000) 配置窗口
  ```
- 启动时只读枚举，打印厂商 ID、设备 ID 与类代码。

**验收**

```text
K3 PCIe: 01:00.0 vendor=... device=... class=0x010802
```

**提交**

```text
feat: read K3 PCIe config space through iATU
```

### 4.3 9B-3：控制器初始化与链路训练

**目标**

由 Alien 完成 APMU 复位、时钟、PERST# 与 LTSSM，PHY 暂时继承 U-Boot。

**改动**

- APMU 基址由 9B-1 取证确定，写入 `k3_riscv/config.rs`，不硬编码猜测值。
- APMU `0x1f0` 位定义：
  | 位 | 含义 |
  |---|---|
  | 6 | `LTSSM_EN` |
  | 9 | `PCIE_AUX_PWR_DET` |
  | 12 | `PCIE_RC_PERST`，0 为高，1 为低 |
  | 30 | `APP_HOLD_PHY_RST` |
  | 31 | `DEVICE_TYPE_RC` |

- APMU `0x1f4` 位定义：
  | 位 | 含义 |
  |---|---|
  | 0 | `PCIE_SOFT_RESET` |
  | 24 | `PCIE_PERSTN_OE` |
  | 25 | `PCIE_PERSTN_OUT` |
  | 31 | `PCIE_IGNORE_PERSTN` |

- 时钟 ID：DBI 89、MSTR 56、SLV 57；复位 ID：DBI 76、MSTR 78、SLV 77。
  时钟门控位从 K3 时钟驱动确认；若 U-Boot 已开启，先只验证复位与 LTSSM。
- 初始化顺序：
  1. 清除 `LTSSM_EN`。
  2. 置 `SOFT_RESET`，延时 2 ms，清除；写后读一次。
  3. 置 `IGNORE_PERSTN | PERSTN_OE`，清 `PERSTN_OUT`，拉低 PERST#。
  4. 使能 DBI/MSTR/SLV 时钟，释放三路复位。
  5. 置 `AUX_PWR_DET`，清 `APP_HOLD_PHY_RST`。
  6. 等待 `T_PVPERL`，约 100 ms。
  7. 置 `PERSTN_OUT | PERSTN_OE`，释放 PERST#。
  8. 设置 `GEN3_EQ_CONTROL_OFF` 的 `PSET_REQ_VEC`。
  9. 写厂商 ID `0x201f`、设备 ID `0x0002`。
  10. 关闭 ASPM L1。
  11. 置 `LTSSM_EN`。
  12. 轮询 link `+0x4` 的 bit1 与 bit12，两者同时置位表示链路已建立。
- 读取 DBI 链路状态，打印速率与宽度。

**验收**

Alien 自主初始化后，配置空间仍可读，链路为 L0；日志包含速率与宽度。

**提交**

```text
feat: initialize K3 PCIe controller and link
```

### 4.4 9B-4：PHY 与分叉

**目标**

不依赖 U-Boot 的 PCIe 初始化，完成 PHY 上电与校准。

**改动**

- 参考主线 K3 组合 PHY 驱动，移植最小初始化与校准序列。
- 从设备树读取 `phys`、`phy-names`、`spacemit,phy-id` 与分叉 GPIO。
- 若 x4 失败，降为 x2；若 Gen3 失败，降为 Gen2/Gen1。
- 失败时打印 PHY 寄存器快照，不 panic。

**验收**

不执行 U-Boot `pci enum`，Alien 也能训练出链路；速率与宽度符合预期或明确降级。

**提交**

```text
feat: bring up K3 PCIe PHY
```

### 4.5 9B-5：BAR、iATU 与完整枚举

**目标**

完成 BAR 分配、出站/入站 iATU 与缓存配置，使 NVMe BAR 可访问。

**改动**

- ATU 视口寄存器：
  | 偏移 | 寄存器 |
  |---|---|
  | `0x900` | `ATU_VIEWPORT` |
  | `0x904` | `ATU_REGION_CTRL1` |
  | `0x908` | `ATU_REGION_CTRL2` |
  | `0x90c` | `LOWER_BASE` |
  | `0x910` | `UPPER_BASE` |
  | `0x914` | `LOWER_LIMIT` |
  | `0x918` | `LOWER_TARGET` |
  | `0x91c` | `UPPER_TARGET` |

- Outbound 0：配置空间，类型 `CFG0`，CPU 基址 `0x11_0000_0000`。
- Outbound 1：BAR0，类型 `MEM`，CPU 基址 `0x11_0011_0000`，
  PCI 目标为 BAR 分配地址，大小 64 KiB。
- Inbound：先读取 U-Boot 留下的视口；若未配置，按 1:1 映射 DRAM。
  K3 内存为 `0x1_0000_0000..0x3_0000_0000`，需要两个 4 GiB 区域。
- 只映射实际需要的 64 KiB，不映射整个 2 GiB 非预取窗口。
- 在 `COHERENCY_CONTROL_3` 中把主控写内存的 `AWCACHE` 设为 `0xf`。

**验收**

```text
K3 PCIe: link up, speed=Gen3, width=4
K3 PCIe: 01:00.0 vendor=... device=... class=0x010802
K3 PCIe: BAR0 size=16 KiB
```

能稳定读取 BAR0 的 `CAP` 寄存器，配置空间读取无异常。

**提交**

```text
feat: map K3 PCIe BAR and DMA windows
```

9B 到此为硬检查点。

## 5. 9C：K3 NVMe 与挂载

### 5.1 9C-1：Identify

**目标**

在 K3 上复用 NVMe 层，读取型号、序列号与容量。

**改动**

- `devices::init_k3_nvme()` 在 `k3` 特性下使用 `SpacemitHost`，
  在 `k3_qemu` 下使用 `EcamHost`。
- 启动顺序：NVMe 先于 SD 与 virtio。
- 失败时打印控制器状态与链路状态，继续启动。

**验收**

型号、序列号、容量与 U-Boot `nvme info` 一致。

**提交**

```text
feat: identify NVMe on K3
```

### 5.2 9C-2：分区与挂载

**目标**

挂载 NVMe 上的 FAT 分区并读取文件。

**改动**

- 复用 9A 的分区解析与 `PartitionBlockDevice`。
- 注册为 `/dev/sda`，挂载到 `/mnt`。
- 若没有 FAT 分区，打印原因并跳过，不 panic。

**验收**

```text
mount fs success at /mnt
Alien:/# /bin/busybox ls /mnt
HELLO.TXT
Alien:/# /bin/busybox cat /mnt/HELLO.TXT
alien k3 nvme mount ok
```

若 NVMe 上没有 FAT 分区，停在“Identify 与原始扇区读取”，
写入测试镜像前单独征得同意。

**提交**

```text
feat: mount FAT from NVMe on K3
```

### 5.3 9C-3：回归与收尾

**目标**

完成单核、双核、八核与回退验证，整理文档。

**验收矩阵**

| 环境 | 预期 |
|---|---|
| K3 单核 | Identify、`/mnt` 挂载、读取成功 |
| K3 双核 | `nproc=2`，NVMe 读取一致 |
| K3 八核 | `8/8 hart`，NVMe 读取一致 |
| K3 无 NVMe | 打印原因，继续启动，SD 卡仍可用 |
| K3 有 NVMe + SD | NVMe 为 `/dev/sda`，SD 跳过并打印说明 |

**收尾**

- 保存关键日志到 `docs/agents/alien/logs/`。
- 更新 `13-pcie-nvme.md`、`03-progress.md`、`README.md`。
- 恢复 `CPU_NUM=1`，重新构建单核最终镜像。
- 确认工作区干净。

**提交**

```text
docs: complete stage 9C K3 nvme
```

## 6. 技术要点

### 6.1 NVMe 协议

- 管理队列与 I/O 队列各 32 项，提交队列 64 字节，完成队列 16 字节。
- 完成轮询检查 phase 位与状态码；状态非零时打印 opcode、CID 与状态。
- 命令超时 1 秒；连续超时后复位控制器并返回错误。
- PRP 使用内核恒等映射地址；`alloc_frames` 返回的地址可直接作为物理地址。
- 先支持 512 字节 LBA。若只有 4096 字节格式，9C 再加只读转换层。

### 6.2 DMA 与缓存

- K3 主控的 `AWCACHE` 设为 `0xf`，按缓存一致处理。
- 先不实现显式缓存维护；若出现队列或数据不一致，再加
  `cbo.flush`/`cbo.inval` 或非缓存映射。
- 该结论需要板卡实测确认，列入风险表。

### 6.3 中断策略

- 使用轮询，不启用 MSI/MSI-X。
- `INTMS` 全部屏蔽；不注册 IMSIC 中断。
- 这样可以避开“IMSIC 只接受 32 位 MSI、硬件不支持 MSI-X”的限制。

### 6.4 设备注册顺序

- `BLOCK_DEVICE` 是 `Once`，第一个成功者成为 `/dev/sda`。
- 阶段 9 先初始化 NVMe，再初始化 SD 与 virtio。
- NVMe 成功时，SD 与 virtio 跳过并打印说明；多设备节点留到阶段 10。

## 7. 代码结构

```text
subsystems/drivers/src/pci.rs           PciHost trait、ECAM、枚举、BAR
subsystems/drivers/src/pci_spacemit.rs  K3 主机桥、iATU、PERST、LTSSM
subsystems/drivers/src/nvme.rs          控制器、队列、Identify、I/O、PRP
subsystems/drivers/src/partition.rs     MBR/GPT、分区偏移包装
subsystems/devices/src/lib.rs           init_k3_nvme、设备注册
kernel/src/lib.rs                       启动顺序：NVMe、SD、virtio
subsystems/platform/src/k3_riscv/config.rs   PCIe/PHY/APMU 地址与 k3_qemu 映射
scripts/alien/run-k3-qemu.sh            QEMU_NVME
docs/agents/alien/13-pcie-nvme.md       本计划与实施记录
```

`drivers` crate 目前没有特性开关。PCIe 与 NVMe 核心可以无条件编译，
平台地址由 `devices` 在 `#[cfg(feature = "k3")]` 下传入，避免新增全局特性。

## 8. 风险与回退

| 风险 | 影响 | 回退 |
|---|---|---|
| U-Boot 也枚举不到 NVMe | 9B 无法开始 | 先检查插槽、供电、分叉与 PERST# |
| PHY 校准序列缺失 | 链路不训练 | 先继承 U-Boot；再移植主线 PHY 序列 |
| T100 IOMMU 已启用 | DMA 访问失败 | 取证后旁路或建立 1:1 映射 |
| iATU 入站映射错误 | NVMe 命令超时 | 对照 U-Boot 视口，先映射小范围 |
| 缓存不一致 | 队列或数据损坏 | 加 `cbo.flush`/`cbo.inval`，或非缓存映射 |
| LBA 为 4096 | 现有块层按 512 处理 | 先只读转换层，或仅支持 512 格式 |
| BAR 为 64 位且窗口过高 | 页表映射过大 | 分配 32 位 BAR 地址，只映射 64 KiB |
| 分叉为 x2 | 带宽下降 | 接受 x2，继续功能验证 |
| U-Boot DHCP 偶发失败 | 上板中断 | 重试上板脚本 |
| 串口抓取偶发错字 | 日志不可靠 | 关键结论以重复读取与寄存器回读为准 |

## 9. 工作量

| 循环 | 工作量 |
|---|---|
| 9A-1 至 9A-5 | 2–3 |
| 9B-1 至 9B-5 | 2–3 |
| 9C-1 至 9C-3 | 1–2 |
| 文档与收尾 | 0.5–1 |
| 合计 | 6–8.5 |

与 `11-roadmap.md` 的 6–8 个单元一致。

## 10. 前置条件与待确认

1. M.2 Key M 插槽中是否已安装 NVMe 硬盘；型号、容量与盘上数据情况。
2. 是否同意只读挂载，不修改盘上内容。
3. 是否可以先做 9B-1 的只读 U-Boot 取证；该操作会复位 PCIe 链路，
   但不写磁盘。
4. 若 NVMe 没有 FAT 分区，是否允许在确认后向空闲区域写入 64 MiB
   测试镜像；否则 9C 只做到 Identify 与原始扇区读取。
5. 板卡上的 M.2 插槽与 Port A 的对应关系，以设备树和丝印核对。

## 11. 参考来源

- QEMU 11.1.2 `virt` 设备树：ECAM `0x3000_0000`、32 位与 64 位 MMIO 窗口。
- Linux `drivers/pci/controller/dwc/pcie-spacemit-k1.c`，K3 支持补丁 v7。
- Linux `include/soc/spacemit/k3-syscon.h`：APMU 寄存器偏移。
- Linux `Documentation/devicetree/bindings/pci/spacemit,k3-pcie-host.yaml`：
  Port A 寄存器、时钟、复位与 `spacemit,apmu` 偏移。
- Linux `phy-spacemit-k3-common.c`、`phy-k3-combo.c`：PHY 初始化与校准。
- NVMe Base Specification 2.x：队列、命令与 PRP。
- 本仓库：`11-roadmap.md`、`12-storage.md`、`01-baseline.md`。
