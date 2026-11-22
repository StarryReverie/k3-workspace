# K3 SDHCI 与 FAT 存储

适用硬件：SpacemiT K3、QEMU `virt`
适用系统：AlienOS `isolation` 的 K3 shell 镜像
适用工具：`nightly-2024-10-25`、U-Boot 2022.10
记录时间：2026-11-22
相关文档：[`k3-aia.md`](k3-aia.md)、[`../testing/k3-storage.md`](../testing/k3-storage.md)、[`../architecture/README.md`](../architecture/README.md)

本文记录 M4 的 SDHCI 块设备域、块栈与 FAT 挂载。驱动使用轮询 PIO，
只读单块访问；不包含 DMA、调谐、UHS 模式与热插拔。

## 硬件

K3 的 SD 控制器是标准 SDHCI：

```text
节点        sdh@d4280000
compatible  spacemit,k3-sdhci
reg         0xd4280000 + 0x200
bus-width   4
```

U-Boot 下的卡信息：

```text
mmc list        sdh@d4280000: 0
mmc info        Capacity: 29.7 GiB, 4-bit
part list mmc 0 Partition 1: start 8192, type 0x0c
```

DTS 中的控制器没有中断要求；M4 使用轮询，`irq = None`。

## 驱动

新增 `domains/drivers/k3_sd`：

- `k3_sd` 实现 `BlkDeviceDomain`，持有 `SafeIORegion` 与分区信息。
- `sdhci.rs` 是旧 `alien/subsystems/drivers/src/sdhci.rs` 的安全 MMIO 移植。
- `gk3_sd` 是域入口，只负责基础设施初始化后调用 `k3_sd::main()`。

初始化流程：

```text
主机复位
POWER_CONTROL = 0x0f      供电 + 3.3V
CLOCK_CONTROL = 400 kHz
CMD0 / CMD8 / ACMD41 / CMD2 / CMD3 / CMD7 / ACMD6 / CMD16
CLOCK_CONTROL = 25 MHz
读取 MBR，选第一个 FAT 分区
校验分区引导扇区 0x55AA
```

三个 K3 细节：

1. `POWER_CONTROL` 必须是 `0x0f`，只写电压位会关闭总线供电。
2. 命令完成必须等 `Normal Interrupt Status` 的 `Command Complete`，不能只看
   `Present State`。
3. 缓冲区就绪位比通用 SDHCI 高两位：可读为 bit11（通用为 bit9）。

`Present State` 在卡插入且空闲时为 `0x1f70000`，`caps=0x35fc0080` 的基础时钟
字段为 0，因此驱动按 200 MHz 计算分频。

## 设备探测与地址

`probe_sd` 在 K3 上扫描 compatible 含 `sdhci` 或 `sdh` 的节点，返回
`CommonDeviceType::SdCard`。启动日志：

```text
sdhci fallback: spacemit,k3-sdhci 0xd4280000-0xd4280200
```

`base/platform/src/k3_riscv/config.rs` 把 `0xd4280000..0xd4290000` 加入
`DEVICE_SPACE`，域才能通过 `SafeIORegion` 访问控制器。

## 块栈

设备树有 SD 卡时，内核创建：

```text
block-1        k3_sd       SDHCI 块设备
shadow_blk-1   shadow_blk  故障恢复/重读层
cache_blk-1    cache_blk   4 KiB 页缓存
/dev/sda       devfs       块设备节点
```

内核在 `init_device` 中查询 `get_capacity()`：

- 容量为 0（无卡、无 MBR、无 FAT 分区）时打印原因，不注册 `block-1`，
  也不创建 shadow/cache 块栈；系统继续启动。
- 容量非 0 时才创建块栈。这样无卡启动不会 panic。

`k3_sd::init` 本身在硬件失败时也返回成功，让内核用容量判断是否可用。

## FAT 挂载

VFS 只在 `cache_blk-1` 存在时挂载 FAT：

| 平台 | 挂载点 | 原因 |
|---|---|---|
| QEMU / VF2 | `/tests` | 与上游基线一致 |
| K3 | `/mnt` | 避免覆盖 initramfs 中的 `/tests/init` 与 `/tests/bash` |

`init_ramfs` 新增 `/mnt` 目录。devfs 在 `cache_blk-1` 存在时创建 `/dev/sda`，
否则打印 `blk domain not found, skip /dev/sda`。

`k3_sd` 的 `write_block` 返回 `EROFS`，M4 保持只读。

## 验证

板端 2 核：

```text
SDHCI: base=0xd4280000 base_clock=200MHz state=0x1f70000 clock=0x0 power=0xf caps=0x35fc0080
SDHCI: card ready rca=0xaaaa v2=true high_capacity=true
SDHCI: partition lba=8192 blocks=62325760 (30432 MiB)
sdcard: 62325760 blocks
fatfs mount success
M4_GLOB: /mnt/HELLO.TXT
M4_FIRST:alien k3 sd mount ok
```

板端 8 核同样挂载成功并读取 `HELLO.TXT`。QEMU K3 无卡时跳过块栈与 FAT，
shell 正常；QEMU virtio 基线的 1/2/4/8 核与磁盘装载全部通过。

## 已知限制

- 只读：`write_block` 返回 `EROFS`。
- 只取 MBR 中第一个 FAT 分区，不创建分区子设备。
- PIO 单块读取，未使用 SDMA/ADMA2。
- 无卡检测中断与热插拔。
- 未做掉电保护与多分区测试。
