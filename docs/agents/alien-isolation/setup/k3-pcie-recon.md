# K3 PCIe 只读取证

适用硬件：SpacemiT K3 参考板（DTB `model = "spacemit k3 com260 ifx board"`）
适用系统：板卡自带 U-Boot 2022.10
适用工具：`scripts/uboot-probe.py`、`nightly-2024-10-25`
记录时间：2026-11-23
相关文档：[`qemu-nvme.md`](qemu-nvme.md)、[`../plan/roadmap.md`](../plan/roadmap.md)、
[`../../alien/13-pcie-nvme.md`](../../alien/13-pcie-nvme.md)

本文记录 M5B 的只读 U-Boot 取证。所有命令均为读取或枚举，不写盘、不改环境、
不保存环境变量。`pci enum` 会复位 PCIe 链路。

## 板卡身份

租用板卡的根节点：

```text
compatible = "spacemit,k3"
model      = "spacemit k3 com260 ifx board"
```

即当前远程板卡是 SpacemiT K3 COM260 IFX 参考板，不是 BananaPi BPI-SM10。
两者都是 K3，但 M.2 槽位与供电设计可能不同。除非确认这块参考板确实插了
NVMe，否则 Link down 属于预期结果。

## 方法

```bash
nix develop -c python3 scripts/uboot-probe.py \
  --command 'pci' --command 'pci enum' --command 'nvme scan' \
  --command 'nvme info' --command 'fdt addr $fdtcontroladdr' \
  --command 'fdt list /soc'
```

再单独读取各 `pcie@`、`pcie-phy@` 节点与 APMU/PHY/link 寄存器。完整节选见
[`../logs/k3-pcie-recon.txt`](../logs/k3-pcie-recon.txt)。

固件版本：

```text
U-Boot 2022.10 (Apr 30 2026 - 11:07:27 +0800), Build: jenkins-BSP-build-bsp-deb-361
```

## 控制器与端口

设备树有 5 个 `spacemit,k3-pcie` 控制器：

| 节点 | 端口 | lanes | status | PHY | 结果 |
|---|---|---|---|---|---|
| `pcie@80000000` | 0 | 4 | okay | PHY0 + PHY1 | `PCIE-0: Link down` |
| `pcie@80400000` | 1 | 2 | disabled | — | 未枚举 |
| `pcie@80800000` | 2 | 2 | disabled | — | 未枚举 |
| `pcie@80c00000` | 3 | 1 | okay | PHY0 | `PCIE-1: Link down` |
| `pcie@81000000` | 4 | 1 | disabled | PHY0 | 未枚举 |

两个已启用端口都没有训练出链路：

```text
pcie_dw_spacemit pcie@80000000: Port0 has no device detect gpio
pcie_dw_spacemit pcie@80000000: pcie config device cmd in rc mode: 0x3bf
PCIE-0: Link down
pcie_dw_spacemit pcie@80c00000: pcie config device cmd in rc mode: 0x33f
PCIE-1: Link down
```

`pci`、`pci enum`、`nvme scan`、`nvme info` 都没有列出任何设备，说明 NVMe
当前对 U-Boot 不可见。

## 寄存器布局

Port0 的 `reg` 与 `reg-names`：

```text
dbi       0x8000_0000 + 0x1000
dbi2      0x8010_0000 + 0x1000
atu       0x8030_0000 + 0x3f20
config    0x11_0000_0000 + 0x1_0000
app       0xd428_29f0 + 0x8
phy_ahb   0x8290_0000 + 0x1000
```

其他字段：

```text
spacemit,pcie-mgmt = <0xd42829d8>
spacemit,pcie-port = <0>
max-link-speed     = <3>
num-lanes          = <4>
ranges             内存窗口 0x11_0011_0000 起约 2 GiB
                   预取窗口 0x18_0000_0000 起 4 GiB
```

Port3（x1）的 `app` 为 `0xd428_29e0`，`phy_ahb` 为 `0x82a0_0000`，
`config` 为 `0x12_8000_0000`，`num-lanes = <1>`，PHY 只引用 `phy0`。

## APMU 与复位状态

APMU 管理基址来自 `spacemit,pcie-mgmt`：`0xd428_29d8`。读取 APP 寄存器：

```text
d42829e0: 0000037f   Port3 APP
d42829e4: 03bf0000
d42829e8: 40000700
d42829ec: 003f0000
d42829f0: 000003ff   Port0 APP
d42829f4: 83bf0000
```

Port0 `0x1f0 = 0x3ff` 说明低 10 位（含 LTSSM_EN 与 AUX_PWR_DET）已置位；
`0x1f4 = 0x83bf0000` 的 bit24/25（PERSTN_OE/OUT）与 bit31（IGNORE_PERSTN）
为 1，说明 PERST# 已释放。

对照主线 `pcie-spacemit-k1.c` 的 APMU 定义：

```text
PCIE_CLK_RESET_CONTROL（+0x0）
  bit6   LTSSM_EN
  bit9   PCIE_AUX_PWR_DET
  bit12  PCIE_RC_PERST        1: assert PERST#
  bit30  APP_HOLD_PHY_RST
  bit31  DEVICE_TYPE_RC       0: endpoint, 1: RC
PCIE_CONTROL_LOGIC（+0x4）
  bit0   PCIE_SOFT_RESET
```

按这个布局，Port0 当前 `LTSSM_EN=1`、`AUX_PWR_DET=1`、`PERST#` 已释放，
但 `DEVICE_TYPE_RC=0`。U-Boot 的 K3 驱动可能有自己的位定义，不能仅凭这一点
断定控制器处在 endpoint 模式；不过它解释了为什么 U-Boot 侧初始化与主线
Linux 的行为可能不同。

主线驱动还要求设备树提供 `vpcie3v3` 稳压器：

```c
ret = devm_regulator_get_enable(dev, "vpcie3v3");
if (ret)
    return dev_err_probe(dev, ret, "failed to get \"vpcie3v3\" supply\n");
```

当前控制 DTB 的 Port0 节点只有 `pinctrl-0`、`phys`、`phy-names`，没有
`vpcie3v3`。这说明这份 DTB 不是按“已供电的 M.2 槽”配置的，槽位 3.3V 是否
上电无法从这里确认。

## PHY 与 link

PHY0（`0x81d0_0000`）前 32 个字：

```text
81d00000: 00000302 00004000 00000979 8f55a98c
81d00010: 00000000 00000000 00000000 03009000
81d00020: 00000000 00000000 0000004a 03020100
81d00030: 07060504 03020100 07060504 0b0a0908
81d00040: a4082001 a8081087 2108318c 18c66318
81d00050: 00006505 03032802 30002000 a0519c80
81d00060: df987810 062888b4 8800a4aa 0000008c
81d00070: 00000120 38500107 00000000 0d0c198f
```

link 寄存器：

```text
82900000: 00008000 0e000000    Port0
82c00000: 00c0a000 88000000    Port1
82d00000: 00c0a000 88000000    Port2
```

PHY 与 link 块可读，但端口未进入 L0。

## 结论

1. 当前租用板卡是 `spacemit k3 com260 ifx board`；两个已启用 PCIe 端口都是
   Link down，U-Boot 枚举不到任何设备。
2. 控制 DTB 没有 `vpcie3v3` 供电描述，且该参考板未必安装 NVMe，因此不能
   把 Link down 归因于某一段驱动代码。
3. Alien 目前无法复用 U-Boot 的链路与 BAR 分配，M5B 的 iATU 访问没有前置
   条件。
4. 需要在装有 NVMe 的板卡上重跑本文命令：确认 M.2 盘插在 x4 槽（Port0）、
   压紧、槽位 3.3V 与 PERST# 正常；并确认设备树是否需要补 `vpcie3v3`。
5. 只有出现 `Link up` 与 `nvme info`，才进入 iATU 配置空间访问与 K3 NVMe
   域。若切换到 BananaPi BPI-SM10，先核对其 DTB 的端口与供电描述。

## 对 M5A 的影响

无影响。QEMU 的 ECAM 与 NVMe 域不依赖 K3 链路；M5A 已完成并通过
1/2/8 核回归。真实 K3 的 NVMe 路径在硬件链路可用后继续。
