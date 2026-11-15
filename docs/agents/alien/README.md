# AlienOS on K3

本目录记录 AlienOS 的调研、本地 QEMU 基线与 K3 移植过程。

## 当前状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 固定 Alien 版本、QEMU 可重复构建 | 完成，2026-11-09 |
| 1 | K3 最小平台移植 | 完成，2026-11-09 |
| 2 | QEMU 回归验证 K3 平台代码 | 完成，2026-11-09 |
| 3 | 开发板首次启动 | 完成，2026-11-10 |
| 4 | K3 控制台交互与 `0x01` 洪泛修复 | 完成，2026-11-14 |
| 5 | FIT gzip 与 K3 双核启动 | 完成，2026-11-14 |
| 6 | K3 四核与八核启动、亲和性与输出锁 | 完成，2026-11-15 |
| 7 | 设备树与中断基础设施 | 计划中，2026-11-15 |

完整 bash 镜像已在 BananaPi-1 上验证；当前支持 gzip FIT 与 8 个 X100 hart。串口无 `0x01` 洪泛。

## 固定信息

```text
仓库    https://github.com/Godones/Alien
提交    181b263b
工具链  nightly-2025-05-20
目标    riscv64gc-unknown-none-elf
```

## 文件

| 文件 | 内容 |
|---|---|
| `01-baseline.md` | 阶段 0 的版本、修复、构建与 QEMU 验收 |
| `02-k3-notes.md` | AlienOS 架构、K3 平台事实与移植计划 |
| `03-progress.md` | 阶段进度与待确认事项 |
| `04-k3-minimal.md` | 阶段 1 的 K3 平台代码、镜像与静态验收 |
| `05-k3-qemu.md` | 阶段 2 的 K3 平台 QEMU 回归 |
| `06-k3-board-boot.md` | 阶段 3 的高地址回归、硬件采集与首次上板 |
| `07-uart-console.md` | 阶段 4 的 `0x01` 定位、轮询 UART 与完整 bash 验证 |
| `08-smp-and-gzip.md` | 阶段 5 的 gzip FIT、双核启动与主 hart 修复 |
| `09-more-cores.md` | 阶段 6 的四核/八核、亲和性、调度竞态与输出锁 |
| `10-interrupts-plan.md` | 阶段 7 的设备树、IMSIC/APLIC 与 UART 中断详细计划 |
| `logs/` | 关键日志节选 |

## 源码位置

Alien 源码已放入仓库根目录的 `alien/`，不含 `.git`。上游 commit 与裁剪范围见
`alien/VENDORED.md`。

## 快速使用

```bash
# 在 k3 仓库根目录，进入 devShell
nix develop

# 构建 QEMU 基线
./scripts/alien/build-qemu.sh

# 构建 K3 最小镜像
./scripts/alien/build-k3.sh

# 构建 K3 双核 gzip 镜像
K3_CPUS=2 ./scripts/alien/build-k3.sh

# 构建 K3 四核或八核 gzip 镜像
K3_CPUS=4 ./scripts/alien/build-k3.sh
K3_CPUS=8 ./scripts/alien/build-k3.sh

# 构建不含 bash 的诊断镜像
K3_DIAG=1 ./scripts/alien/build-k3.sh

# 构建 UART 输入自测镜像
K3_DIAG=1 K3_UARTTEST=1 ./scripts/alien/build-k3.sh

# 上板启动诊断镜像
./scripts/alien/boot-k3-board.py

# 上板并发送后续命令
./scripts/alien/boot-k3-board.py \
  --image alien/out-k3-diag/alien-k3-diag.itb \
  --marker K3_UART_READY \
  --post-command "uart-ok"

# 构建 K3 平台的 QEMU 回归镜像
./scripts/alien/build-k3-qemu.sh

# 运行 K3 平台 QEMU 回归
QEMU_TIMEOUT=30 ./scripts/alien/run-k3-qemu.sh

# 构建带 bash 的 QEMU 交互镜像
K3_QEMU_SHELL=1 ./scripts/alien/build-k3-qemu.sh

# 构建并运行 QEMU 双核回归
K3_CPUS=2 K3_QEMU_SHELL=1 ./scripts/alien/build-k3-qemu.sh
QEMU_SMP=2 QEMU_TIMEOUT=10 ./scripts/alien/run-k3-qemu.sh

# 构建并运行 QEMU 八核回归
K3_CPUS=8 K3_QEMU_SHELL=1 ./scripts/alien/build-k3-qemu.sh
QEMU_SMP=8 QEMU_TIMEOUT=15 ./scripts/alien/run-k3-qemu.sh

# 启动 QEMU，默认 60 秒后停止
./scripts/alien/run-qemu.sh

# 不自动停止
QEMU_TIMEOUT=0 ./scripts/alien/run-qemu.sh
```

需要改用其他 Alien 源码目录时，设置 `ALIEN_DIR`：

```bash
ALIEN_DIR=/path/to/alien ./scripts/alien/build-k3.sh
```

`build-qemu.sh` 不使用 Alien 自带的 Makefile。该 Makefile 依赖 `sudo mount`、交互式 BusyBox 配置与 `gen_ksym`，不适合当前环境。
