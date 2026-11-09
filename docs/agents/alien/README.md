# AlienOS on K3

本目录记录 AlienOS 的调研、本地 QEMU 基线与 K3 移植过程。

## 当前状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 固定 Alien 版本、补丁、QEMU 可重复构建 | 完成，2026-11-09 |
| 1 | K3 最小平台移植 | 完成，2026-11-09 |
| 2 | QEMU 回归验证 K3 平台代码 | 完成，2026-11-09 |
| 3 | 开发板首次启动 | 未开始 |

尚未连接开发板，尚未向开发板上传文件。

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
| `01-baseline.md` | 阶段 0 的版本、补丁、构建与 QEMU 验收 |
| `02-k3-notes.md` | AlienOS 架构、K3 平台事实与移植计划 |
| `03-progress.md` | 阶段进度与待确认事项 |
| `04-k3-minimal.md` | 阶段 1 的 K3 平台代码、镜像与静态验收 |
| `05-k3-qemu.md` | 阶段 2 的 K3 平台 QEMU 回归 |
| `patches/` | 相对 Alien 提交 `181b263` 的补丁 |
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

# 构建 K3 平台的 QEMU 回归镜像
./scripts/alien/build-k3-qemu.sh

# 运行 K3 平台 QEMU 回归
QEMU_TIMEOUT=30 ./scripts/alien/run-k3-qemu.sh

# 启动 QEMU，默认 60 秒后停止
./scripts/alien/run-qemu.sh

# 不自动停止
QEMU_TIMEOUT=0 ./scripts/alien/run-qemu.sh
```

需要改用其他 Alien 源码目录时，设置 `ALIEN_DIR`：

```bash
ALIEN_DIR=/path/to/alien ./scripts/alien/build-k3.sh
```

`apply-patches.sh` 只用于从上游重建参考树，不再参与日常构建。

`build-qemu.sh` 不使用 Alien 自带的 Makefile。该 Makefile 依赖 `sudo mount`、交互式 BusyBox 配置与 `gen_ksym`，不适合当前环境。
