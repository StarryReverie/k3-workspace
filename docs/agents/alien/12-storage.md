# 阶段 8 实施记录：存储与块设备

日期：2026-11-16
状态：完成（8A–8D 均已完成）
范围：QEMU virtio-blk 与 FAT、K3 SDHCI 块设备
相关计划：`11-roadmap.md`

## 1. 结论

- 8A 完成：`k3_qemu` 可以用 virtio-blk 挂载 FAT32，读写文件正常。
- 8B 完成：K3 的 SD 卡、分区表、控制器节点与时钟现状已取证。
- 8C 完成：SDHCI 驱动识别卡、解析 MBR，挂载 FAT32 分区到 `/mnt`，
  文件读取正常。
- 8D 完成：K3 单核与八核回归通过，日志与文档已归档。

## 2. QEMU：virtio-blk 与 FAT

### 2.1 构建

`build-k3-qemu.sh` 新增 `K3_QEMU_DISK=1`：

- 特性追加 `fat`。
- initramfs 只保留 `bin/busybox`。
- 生成 72 MiB FAT32 镜像，写入 `init`、`bash`、`libc.so`。
- 挂载点为 `/tests`，与上游 QEMU 基线一致。

`run-k3-qemu.sh` 新增 `QEMU_DISK`：

```bash
K3_CPUS=2 K3_QEMU_SHELL=1 K3_QEMU_DISK=1 ./scripts/alien/build-k3-qemu.sh
QEMU_DISK=1 QEMU_SMP=2 QEMU_TIMEOUT=30 ./scripts/alien/run-k3-qemu.sh
```

### 2.2 结果

```text
Init block device, base_addr:0x10008000,irq:8
Init block device success
mount fs success at /tests

Alien:/# /bin/busybox ls /tests
bash     init     libc.so
Alien:/# echo disk_rw_ok > /tests/probe.txt
Alien:/# /bin/busybox cat /tests/probe.txt
disk_rw_ok
```

AIA 模式下同样可用，中断控制台不受影响。

## 3. K3 取证

U-Boot 命令与结果：

```text
mmc list
  sdh@d4280000: 0

mmc info
  Manufacturer ID: 3
  Name: SK32G
  Bus Speed: 204800000
  Capacity: 29.7 GiB
  Bus Width: 4-bit

part list mmc 0
  Partition Type: DOS
  Part  Start Sector  Num Sectors  Type
  1     8192          62325760     0c
```

设备树节点：

```text
sdh@d4280000 {
    compatible = "spacemit,k3-sdhci";
    reg = <0x0 0xd4280000 0x0 0x200>;
    bus-width = <0x4>;
    clock-frequency = <0xc350000>;   /* 200 MHz */
    spacemit,tx_delaycode = <0x1f>;
    sdh-phy-module = <0x0>;
}
```

关键寄存器现状：

| 寄存器 | 值 | 含义 |
|---|---|---|
| `caps` | `0x35fc0080` | 基础时钟字段为 0，需按 200 MHz 处理 |
| `host2` | `0x0` | 未启用 1.8V 信号 |
| `clock` | `0x0` | U-Boot 退出时关闭了 SD 时钟 |
| `power` | `0xf` | 供电使能 + 3.3V |
| `present state` | `0x1f70000` | 卡已插入、状态稳定、DAT/CMD 线空闲高电平 |

## 4. K3 SDHCI 驱动

### 4.1 实现

`subsystems/drivers/src/sdhci.rs`：

- 主机复位、上电、时钟配置（400 kHz 初始化，25 MHz 数据传输）。
- 卡初始化：CMD0、CMD8、ACMD41、CMD2、CMD3、CMD7、ACMD6、CMD16。
- 单块 PIO 读写，轮询完成，无 DMA、无调谐、无中断。
- 读取 MBR，选取第一个 FAT 分区，按分区偏移读写。
- 挂载前校验分区引导扇区，未格式化时跳过而不是 panic。

### 4.2 三个硬件细节

这些细节都通过寄存器读回确认：

1. **供电位**

   `POWER_CONTROL` 的 bit0 是总线供电使能，电压在 bits 3:1。
   `0x0f` 才表示“供电 + 3.3V”，只写 `0x0e` 会关闭供电，卡完全不响应。

2. **命令完成**

   写命令后必须等待 `Normal Interrupt Status` 的 `Command Complete`
   位，只依赖 `Present State` 的 `Command Inhibit` 会读到尚未更新的响应。

3. **缓冲区就绪位**

   该控制器的 `Present State` 与通用 SDHCI 规范相差两位：

   | 含义 | 通用 SDHCI | 本控制器 |
   |---|---|---|
   | 缓冲区可写 | bit8 | bit10 |
   | 缓冲区可读 | bit9 | bit11 |
   | 读传输进行中 | — | bit9 |

   U-Boot 的 `SDHCI_DATA_AVAILABLE` 同样是 `0x800`，与该结论一致。

### 4.3 结果

```text
SDHCI: base=0xd4280000 base_clock=200MHz host2=0x0 state=0x1f70000 clock=0x0 power=0xf caps=0x35fc0080
SDHCI: card ready rca=0xaaaa v2=true high_capacity=true
SDHCI: partition lba=8192 blocks=62325760 (30432 MiB)
K3 storage: partition blocks=62325760
mount fs success at /mnt

Alien:/# /bin/busybox ls /mnt
HELLO.TXT
Alien:/# /bin/busybox cat /mnt/HELLO.TXT
alien k3 sd mount ok
```

读取稳定：同一扇区重复读取结果一致，MBR 内容与 U-Boot 的 `mmc info`
和 `part list` 完全吻合。U-Boot 在 `mmc write` 之后会留下 1.8V 信号与
开启的时钟，驱动通过主机复位与重新上电回到默认状态。

## 5. 分区格式化

SD 卡的分区 1 起始于 8192 号扇区、类型为 `0x0c`，但该扇区原本全零，
分区没有 FAT 文件系统。写入前先抽样确认前 64 MiB 为空，然后：

1. 宿主机生成 48 MiB FAT32 镜像，放入 `HELLO.TXT`。
2. 通过 TFTP 加载到 `0x150000000`。
3. `mmc write 0x150000000 0x2000 0x18000` 写入分区 1。
4. `mmc read` 与 `md.l` 校验，读到 `EB 58 90` 与 `mkfs.fat` 字样。

写入影响范围是分区 1 的前 48 MiB，占该分区约 0.16%，不触碰 MBR 与
其余区域。该流程已纳入上板脚本：

```bash
./scripts/alien/boot-k3-board.py   --image alien/out-k3/alien-k3.itb   --sd-image alien/out-sd-test/sdcard.img   --sd-start 0x2000
```

`--sd-image` 会在启动前完成上传、写入与校验；`--sd-sectors` 默认为
镜像大小除以 512。

## 6. 验证矩阵

| 环境 | 结果 |
|---|---|
| QEMU PLIC + 磁盘 | `/tests` 列出文件，写入与读回正常 |
| QEMU AIA + 磁盘 | 同上，中断控制台正常 |
| QEMU 无磁盘 | 回退 initramfs，正常启动 |
| K3 单核 | `/mnt` 挂载，读取 `HELLO.TXT` 成功 |
| K3 八核 | `8/8` hart，`/mnt` 挂载与读取正常，`42` 中断计数递增 |
| K3 无卡或无有效分区 | 打印原因后继续启动，无 panic |

## 7. 修改文件

```text
scripts/alien/build-k3-qemu.sh              K3_QEMU_DISK 模式
scripts/alien/run-k3-qemu.sh                QEMU_DISK
subsystems/devices/src/lib.rs               init_k3_virtio_block、init_k3_storage
subsystems/drivers/src/sdhci.rs             SDHCI 驱动
subsystems/drivers/src/lib.rs               注册 sdhci 模块
subsystems/kernel/src/lib.rs                启动顺序
subsystems/platform/src/*/config.rs         DISK_MOUNT，K3 增加 SDHCI 地址
subsystems/vfs/src/lib.rs                   按平台挂载点挂载
scripts/alien/build-k3.sh                   启用 fat 特性
```

## 8. 后续

1. 写入支持：当前驱动已实现单块写，但尚未做完整写入测试与掉电保护。
2. 传输方式：使用 SDMA 或 ADMA2 替代 PIO，提高吞吐并消除缓冲区竞态。
3. 分区层：支持多分区与分区设备节点，而不是只取第一个 FAT 分区。
4. 热插拔与卡检测：使用 DT 中的 `cd-gpios`。
5. 文件系统：按需增加 ext4 支持。
6. 阶段 9：PCIe 与 NVMe，见 `11-roadmap.md`。
