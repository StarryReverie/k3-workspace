# AlienOS isolation on K3

本目录记录 AlienOS `isolation` 分支的源码导入、构建基线与 K3 隔离域移植。
旧的 `alien/` 目录继续保留，作为宏内核版本的参考，不与本目录的结论混用。

## 当前状态

| 项目 | 状态 |
|---|---|
| `main` 合并旧 `alien` | 完成，快进合并 |
| vendor Alien `isolation` 与两个子模块 | 完成 |
| 统一 devShell 工具链 | 完成，`nightly-2024-10-25` |
| QEMU 基线构建与启动 | 完成，26 init 域、8 disk 域、shell 可用 |
| QEMU 回归矩阵 | 完成，1/2/4/8 核、磁盘域动态装载、日志稳定 |
| K3 诊断内核 | 完成，高地址 QEMU 与真实板卡 1/2/8 核 |
| K3 轮询控制台与 shell | 完成，QEMU 1/2/8 核；板端 2 核交互、8 核到提示符 |
| K3 AIA 中断 | 待开始 |

## 固定信息

```text
Alien       https://github.com/Godones/Alien
分支        isolation
提交        e37137b15093c379d735619f0be18c2d9ff2a70a

domains     d4d6d8ee95fce96ed856b63c2775b40d38dda67c
domain-lib  136007cdca82531f97a4e8da082b2ce48b839d2f
```

源码位于仓库根目录的 `alien-isolation/`。`domains/` 与 `domain-lib/` 已展开为
普通目录，不再使用 Git submodule。第三方 crate 不做 vendor，由 `Cargo.lock`
固定版本。

## 目录

| 路径 | 内容 |
|---|---|
| [`architecture/README.md`](architecture/README.md) | 隔离内核启动路径、平台接口与域依赖 |
| [`setup/vendoring.md`](setup/vendoring.md) | 源码版本、目录约定、裁剪范围与更新方式 |
| [`setup/toolchain.md`](setup/toolchain.md) | Rust 与交叉编译工具链 |
| [`setup/qemu-baseline.md`](setup/qemu-baseline.md) | QEMU 构建、启动与 shell 验收 |
| [`setup/k3-platform.md`](setup/k3-platform.md) | K3 平台配置、诊断镜像与 FIT |
| [`setup/k3-console.md`](setup/k3-console.md) | K3 轮询 UART、域调整与多核启动修复 |
| [`testing/qemu-matrix.md`](testing/qemu-matrix.md) | 1/2/4/8 核回归、磁盘域动态装载与日志稳定性 |
| [`testing/k3-qemu.md`](testing/k3-qemu.md) | K3 诊断内核的高地址 QEMU 回归 |
| [`testing/k3-board.md`](testing/k3-board.md) | K3 诊断内核的真实板卡启动 |
| [`testing/k3-shell.md`](testing/k3-shell.md) | K3 shell 的 QEMU 与板端回归 |
| [`plan/roadmap.md`](plan/roadmap.md) | 从 vendor 到 K3 PCIe/NVMe 的粗略路线 |

新文档按主题放入子目录，不使用数字前缀。

## 与旧记录的关系

`docs/agents/alien/` 记录宏内核路径的 QEMU 基线、K3 平台、AIA、SDHCI 与
NVMe 工作。该目录中的平台事实、驱动协议与测试思路仍可参考，但代码结构不能
直接套用到隔离域架构。
