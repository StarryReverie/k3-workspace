# K3 高地址 QEMU 回归

适用硬件：QEMU `virt`，10 GiB RAM，1/2/8 个 hart
适用系统：AlienOS `isolation` 的 K3 诊断内核
适用工具：`nightly-2024-10-25`、QEMU 9 及以上
记录时间：2026-11-22
相关文档：[`../setup/k3-platform.md`](../setup/k3-platform.md)、[`../architecture/README.md`](../architecture/README.md)

本文记录 K3 诊断内核在高地址 QEMU 上的测试入口、断言与结果。测试不启动域，
不验证真实 K3 固件、AIA 或 UART。

## 场景

| 场景 | 核数 | 验证内容 |
|---|---|---|
| `k3-1` | 1 | 单 hart 完成 TCB 初始化 |
| `k3-2` | 2 | 1 个从 hart 启动 |
| `k3-8` | 8 | 7 个从 hart 启动 |
| `all` | — | 依次执行以上三个场景 |

## 构建与运行

```bash
nix develop

# 构建并运行全部场景
./scripts/alien-isolation/test-k3-qemu.py --build all

# 只运行已有镜像
./scripts/alien-isolation/test-k3-qemu.py --no-build k3-8
```

单独构建与运行：

```bash
./scripts/alien-isolation/build-k3.sh --smp 8
./scripts/alien-isolation/run-k3-qemu.sh --smp 8
```

镜像位于 `alien-isolation/build/k3/smp-N/`，日志位于 `logs/tests/`。

## 断言

测试按以下顺序检查：

1. `K3_BOOT_OK`。
2. `Memory:` 的上界为 `0x300000000`。
3. `Relocate initrd data to 0x...`。
4. `Boot hart N`，以及其余每个 hart 的 `hart N start...`。
5. 串口输出不得包含 `Kernel panic` 或 `panicked at`。

启动 hart 由 QEMU 决定，可能是 0，也可能不是。测试按实际 `Boot hart` 计算
应出现的从 hart 集合。

## 验收结果

2026-11-22 执行：

```bash
./scripts/alien-isolation/test-k3-qemu.py --build all
```

结果：

| 场景 | 启动 hart | 从 hart | 日志大小 | 结果 |
|---|---|---|---|---|
| `k3-1` | 0 | 无 | 3.9 KB | 通过 |
| `k3-2` | 0 | 1 | 3.9 KB | 通过 |
| `k3-8` | 1 | 0、2、3、4、5、6、7 | 4.0 KB | 通过 |

完整输出：

```text
PASS alien-isolation k3 qemu: 3 scenarios
```

8 核场景的启动 hart 在另一次手工运行中为 0。启动 hart 的分配不应被假设。

日志节选见 [`../logs/k3-qemu.txt`](../logs/k3-qemu.txt)。

## 已知限制

- 只覆盖 QEMU `virt` 的 DTB 与 SBI，不覆盖 K3 固件。
- 不初始化 IMSIC、APLIC 或 PLIC。
- 不启动域，不验证用户态、VFS 与调度。
- 不验证 K3 UART 的 XScale 寄存器模型与中断。
- FIT 上板验证见 [`k3-board.md`](k3-board.md)。
