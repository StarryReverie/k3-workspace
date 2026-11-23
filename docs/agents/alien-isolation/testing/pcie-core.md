# PCIe 核心单元测试

适用硬件：无（主机侧）
适用系统：AlienOS `isolation` 的 `pci-core`
适用工具：`nightly-2024-10-25`
记录时间：2026-11-23
相关文档：[`../setup/k3-pcie.md`](../setup/k3-pcie.md)、
[`../setup/qemu-nvme.md`](../setup/qemu-nvme.md)

本文记录在无 K3 板卡、无 NVMe 盘时对 PCIe 代码路径做的离硬件验证。

## 命令

```bash
nix develop -c cargo test -p pci-core
```

结果：

```text
running 6 tests
test tests::config_access_programs_outbound_zero ... ok
test tests::detects_unrolled_and_viewport ... ok
test tests::enumerates_ecam_devices_and_sizes_bar ... ok
test tests::link_state_uses_bits_one_and_twelve ... ok
test tests::map_bar_programs_outbound_one ... ok
test tests::viewport_config_access_programs_target ... ok
test result: ok. 6 passed
```

## 覆盖范围

| 测试 | 验证内容 |
|---|---|
| `enumerates_ecam_devices_and_sizes_bar` | bus 0 扫描、host bridge 与 NVMe 识别、BAR 大小探测、BAR 分配、Memory/Bus Master |
| `detects_unrolled_and_viewport` | DBI `0x900` 读回全 1 判为 unrolled，否则判为 viewport |
| `config_access_programs_outbound_zero` | unrolled 模式下每次配置访问都重写 Outbound 0，目标为 `dev << 15` |
| `viewport_config_access_programs_target` | viewport 模式下先写 `0x900` 选 region，再经 `0x904+reg` 写目标地址 |
| `link_state_uses_bits_one_and_twelve` | `PHY_AHB_LINK_STS` 的 bit1 与 bit12 必须同时置位 |
| `map_bar_programs_outbound_one` | BAR 映射写入 Outbound 1 的 CPU 基址、大小、PCI 目标与 enable 位 |

## 模型

- `FakeEcam`：一页扁平配置空间，模拟 host bridge 与 00:01.0 NVMe，
  包含 16 KiB 可探测 BAR 与 Command 寄存器。
- `FakeDw`：DBI/ATU/link/config 四组寄存器数组，模拟 unrolled iATU、
  Outbound 0/1 的写入顺序、`CTRL2.ENABLE` 回读与 link 状态。

## 边界

这些测试证明的是**寄存器序列与地址计算**，不覆盖：

- APMU 时钟/复位、PERST#、PHY 校准与真实 LTSSM；
- 真实设备 DMA 与缓存一致性；
- QEMU NVMe 的完成项时序（由集成测试与重试覆盖）。

因此它不能替代 M5B/M5C 的板端验收，但能在没有盘的情况下防止 iATU/BAR
逻辑回退。
