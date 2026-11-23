# 干净构建与全矩阵

适用硬件：主机、QEMU `virt`、BananaPi（SpacemiT K3）
适用系统：AlienOS `isolation`
适用工具：`nightly-2024-10-25`、QEMU 9 及以上
记录时间：2026-11-24
相关文档：[`qemu-matrix.md`](qemu-matrix.md)、[`k3-shell.md`](k3-shell.md)、
[`k3-storage.md`](k3-storage.md)、[`domain-recovery.md`](domain-recovery.md)、
[`../plan/roadmap.md`](../plan/roadmap.md)

本文记录 M6 的干净构建与完整回归结果。

## 1. 清理范围

```bash
rm -rf alien-isolation/build \
       alien-isolation/target \
       alien-isolation/domains/target \
       alien-isolation/domains/common/*/target
```

这清掉了内核、用户程序、域与所有镜像。第三方 crate 仍来自本地 Cargo
缓存或网络，因此这不是完全离线验证；`setup/vendoring.md` 已说明该边界。

## 2. 结果

| 项目 | 命令 | 结果 |
|---|---|---|
| PCIe 单元测试 | `cargo test -p pci-core` | 6 passed |
| QEMU 矩阵 | `test-qemu.py --build all` | 7/7 |
| K3 矩阵 | `test-k3-shell.py --build all` | 6/6 |
| 板端 2 核 | `boot-k3-board.py` + smp-2 镜像 | 通过 |
| 板端 8 核 | `boot-k3-board.py` + smp-8 镜像 | 通过（重试一次） |

### 2.1 QEMU 7 场景

```text
PASS smp-1
PASS smp-2
PASS smp-4
PASS smp-8
PASS disk-load
PASS blk-crash
PASS block-swap
```

前五个是 M1 基线，后两个是 M6 新增的隔离性场景，机制见
[`domain-recovery.md`](domain-recovery.md)。

### 2.2 K3 6 场景

```text
PASS k3-shell-1
PASS k3-shell-2
PASS k3-shell-8
PASS k3-nvme-1
PASS k3-nvme-2
PASS k3-nvme-8
```

这六个场景跑在高地址 QEMU 上，使用 K3 平台配置与 NVMe 测试镜像。

### 2.3 板端证据

2 核与 8 核都在真实 K3 板卡上启动，PCIe 仍为 Link down 降级，SD 卡读取正常：

```text
Boot hart 0
K3 PCIe: iATU unrolled
K3 PCIe: port=0 link down, PHY_AHB_LINK_STS=0xe000000, skip NVMe
SDHCI: partition lba=8192 blocks=62325760 (30432 MiB)
fatfs mount success
M6_BOARD:alien k3 sd mount ok
M6_BOARD8:alien k3 sd mount ok
```

## 3. 8 核串口丢行

第一次跑 `k3-nvme-8` 时断言 `hart 6 start...` 超时，但该 hart 实际已经运行：

```text
[6] line 32, file kernel/src/panic.rs: Test panic
[6] Test no panic...
```

原因是八个 hart 同时输出时，`hart 6 start...` 这一行被串口丢弃或与相邻行
粘连。板端 8 核日志里也有类似现象。

测试侧改为 `QemuSession.wait_for_harts`：每个 hart 只要出现
`hart N start...` 或 `[N] Test no panic...` 之一，就认为启动成功。QEMU 与
K3 两个测试脚本共用该判定。

## 4. 板端基础设施

8 核第一次启动失败，串口停在 U-Boot 网络阶段：

```text
EQOS_DMA_MODE_SWR stuck
serial websocket read failed: Connection reset
```

内核尚未加载，属于已知的板卡会话抖动。重试同一镜像后通过，因此不计入代码
失败。

## 5. 结论

- 干净构建可以从空 `target/` 与 `build/` 完成，无缺失文件。
- QEMU 7 场景、K3 6 场景、`pci-core` 6 个单元测试全部通过。
- 板端 2/8 核启动、SD 读取与 PCIe 降级保持 M4/M5B 的行为。
- 两个新增场景确实覆盖了域 panic 恢复与热替换，不是空断言。
