# AlienOS on K3

本目录记录 AlienOS 的调研、本地 QEMU 基线与 K3 移植过程。

## 当前状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 固定 Alien 版本、补丁、QEMU 可重复构建 | 完成，2026-11-09 |
| 1 | K3 最小平台移植 | 完成，2026-11-09 |
| 2 | QEMU 回归验证 K3 平台代码 | 未开始 |
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
| `patches/` | 相对 Alien 提交 `181b263` 的补丁 |
| `logs/` | 关键日志节选 |

## 快速使用

```bash
# 在 k3 仓库根目录，进入 devShell
nix develop

# 克隆 Alien、检出固定提交并应用补丁
./scripts/alien/apply-patches.sh /tmp/opencode/Alien-clean

# 构建 QEMU 基线
ALIEN_DIR=/tmp/opencode/Alien-clean ./scripts/alien/build-qemu.sh

# 构建 K3 最小镜像
ALIEN_DIR=/tmp/opencode/Alien-clean ./scripts/alien/build-k3.sh

# 启动 QEMU，默认 60 秒后停止
ALIEN_DIR=/tmp/opencode/Alien-clean ./scripts/alien/run-qemu.sh

# 不自动停止
ALIEN_DIR=/tmp/opencode/Alien-clean QEMU_TIMEOUT=0 ./scripts/alien/run-qemu.sh
```

`build-qemu.sh` 不使用 Alien 自带的 Makefile。该 Makefile 依赖 `sudo mount`、交互式 BusyBox 配置与 `gen_ksym`，不适合当前环境。
