# K3 PCIe 控制器接入

适用硬件：SpacemiT K3（DesignWare 主机桥）
适用系统：AlienOS `isolation` 的 K3 shell 镜像
适用工具：`nightly-2024-10-25`
记录时间：2026-11-23
相关文档：[`k3-pcie-recon.md`](k3-pcie-recon.md)、[`qemu-nvme.md`](qemu-nvme.md)、
[`../testing/qemu-nvme.md`](../testing/qemu-nvme.md)

本文记录 M5B-0：K3 PCIe 的 TCB 代码路径。真实链路当前仍为 Link down，
因此代码在链路不通时会安全降级并回退到 SD 卡。

## 与 QEMU 的区别

| 项目 | QEMU `virt` | K3 |
|---|---|---|
| 主机桥 | `pci-host-ecam-generic` | `spacemit,k3-pcie` |
| 配置空间 | ECAM 直接寻址 | iATU Outbound 0 + 固定 CPU 窗口 |
| BAR 分配 | TCB 写配置空间 | TCB 写配置空间，再用 iATU Outbound 1 映射 |
| 链路 | 无须训练 | APMU/PHY/PERST#/LTSSM |
| 地址重叠 | 无 | DBI `0x8000_0000` 与 QEMU RAM 基址重叠 |

因此 K3 的 PCIe 地址不能无条件放进 `DEVICE_SPACE`。

## 设备树解析

`probe_pci` 先找 ECAM，再找 `spacemit,k3-pcie`。K3 节点的 `reg` 顺序为：

```text
dbi, dbi2, atu, config, app, phy_ahb
```

`reg-names` 优先，缺失时按位置回退。选择规则：

1. `status = "okay"`。
2. 取 `num-lanes` 最大的端口（当前 Port0 为 x4）。
3. 记录 `spacemit,k3-pcie` 的 `reg` 与 `num-lanes`、`spacemit,pcie-port`。

结果放进 `PciHostInfo { ecam: None, spacemit: Some(...) }`，由
`pci_init` 分派。

## 运行期映射

`platform::is_k3_board()` 检查 DTB 根节点 compatible 是否包含
`spacemit,k3`。只有真实 K3 才映射 `K3_PCIE_SPACE`：

```text
pcie-dwc      0x8000_0000 + 0x02e0_0000   DBI/DBI2/ATU/link/PHY
pcie-config0  0x11_0000_0000 + 0x1_0000   Port0 配置窗口
pcie-bar0     0x11_0011_0000 + 0x10_0000  Port0 BAR 窗口
apmu-pcie     0xd428_2000 + 0x1000        APMU 所在整页
```

QEMU 的根节点是 `riscv-virtio,qemu`，因此不会安装这些映射，避免覆盖
QEMU 从 `0x8000_0000` 开始的 RAM。

映射在 `build_kernel_address_space` 中完成，使用 `VmAreaEqual`。所有区间
必须页对齐；APMU 基址 `0xd4282800` 不是页对齐，所以取
`0xd4282000..0xd4283000`。

## iATU

`SpacemitHost::new` 读取 DBI `0x900`：

- 读到 `0xffff_ffff`：unrolled 布局，每个 region 占 512 字节，入站在
  偏移 `+0x100`，寄存器相对偏移 `0x0/0x4/0x8/...`。
- 其他值：viewport 布局，先写 `0x900` 选择 region，再访问 `0x904+reg`。

当前板卡是 **unrolled**。

配置空间访问每次重写 Outbound 0：

```text
CTRL1       CFG0
LOWER_BASE  配置窗口 CPU 基址
LIMIT       基址 + 0x10000 - 1
TARGET      (bus << 20) | (dev << 15) | (func << 12)
CTRL2       ENABLE
```

随后读 `config_base + (offset & 0xffc)`。写配置空间同理。每次写完
`CTRL2` 轮询 enable 位，最多 1000 次。

链路状态读 `phy_ahb + 0x4`（`PHY_AHB_LINK_STS`）：

```text
bit1   SMLH_LINK_UP
bit12  RDLH_LINK_UP
```

两者同时置位才算 Link up。

## BAR 与 NVMe 域

链路 up 后：

1. 扫描 bus 0，找 class `0x010802`。
2. 在 `K3_PCI_BAR_WINDOW_START` 内按 BAR 大小对齐分配，写回配置空间。
3. 打开 Memory Space 与 Bus Master。
4. 用 Outbound 1 把 CPU 窗口映射到 BAR 的 PCI 地址。
5. 把 CPU 窗口区间交给现有 `nvme` 域。

NVMe 域不需要改动；它只看到一段可访问的 BAR。

## 链路不通时

当前板卡两个已启用端口都是 Link down，因此 `init_spacemit` 打印：

```text
K3 PCIe: port=0 lanes=4 dbi=0x80000000-0x80001000 atu=0x80300000-0x80303f20 config=0x1100000000-0x1100010000
K3 PCIe: iATU unrolled
K3 PCIe: port=0 link down, PHY_AHB_LINK_STS=0xe000000, skip NVMe
```

然后不创建 NVMe 域，继续启动，SDHCI/FAT 路径不受影响。

## QEMU NVMe 稳定性

把 NVMe 驱动从旧树移植到域后，QEMU 上出现过偶发完成项异常：设备把一条
有效命令的完成项写成 `CID=0`，读文件因此失败。

已做的三件事：

1. 在 `arch` 增加 `fence_io()`（`fence iorw, iorw`）。检查产物发现
   `core::sync::atomic::fence(SeqCst)` 在自定义 target 上只生成
   `fence rw,w` / `fence r,rw`，不覆盖设备 I/O。
2. SQE 改为 8 字节 volatile 写入，保证 opcode 与 CID 同时可见。
3. 读操作是幂等的：CID 不符时重新提交（最多 3 次）；控制器初始化失败时
   整体复位重试（最多 3 次）。

压测 20 次读文件全部通过，其中一次初始化阶段命中异常，复位重试后成功。
该异常尚未在真实 K3 上复现条件（链路未通），继续保留诊断输出。

## 代码位置

```text
base/pci-core/                        通用枚举、BAR、iATU（可主机侧测试）
kernel/src/bus/pci/mod.rs             平台策略、真实 MMIO、NVMe BAR 交接
kernel/src/bus/fdt.rs                 spacemit,k3-pcie 解析
base/mem/src/vmm.rs                   板卡条件映射
base/platform/src/k3_riscv/config.rs  K3 PCIe/APMU 地址
base/platform/src/k3_riscv/mod.rs     is_k3_board
domain-lib/arch/src/riscv/mod.rs      fence_io
domains/drivers/nvme/                 NVMe 域与重试
```

## 离硬件验证

`pci-core` 通过 `DwIo` 与 `PciHost` 抽象 MMIO，不依赖 K3 硬件。主机侧
单元测试覆盖枚举、BAR 探测/分配、unrolled/viewport 判定、配置目标重写、
Outbound 1 映射与 link 位定义：

```bash
nix develop -c cargo test -p pci-core
```

详见 [`../testing/pcie-core.md`](../testing/pcie-core.md)。

## 下一步

1. 在装有 NVMe 的板卡上确认 `Link up`。
2. 核对 BAR 分配与 iATU Outbound 1 的实际读写。
3. 复用 `nvme` 域完成 Identify、分区与 `/mnt` 挂载。
4. 若 U-Boot 能枚举但 Alien 不能，先对照 U-Boot 的 iATU 视口与 BAR。
