# Vendored Alien

Upstream: `https://github.com/Godones/Alien`
Commit:   `181b263bbcaeb3e75340f7b1a3fd224ec4757b7e`
Date:     2026-11-09

本目录是 Alien 源码的必要子集，不包含 `.git`。构建入口仍使用仓库根目录的
`scripts/alien/`。

## 已包含

- `kernel/`
- `subsystems/`
- `user/`
- `Cargo.toml`、`Cargo.lock`
- `LICENSE`、`README.md`
- `rust-toolchain.toml`、`rustfmt.toml`
- `.cargo/config.toml`
- `.gitignore`
- `tools/linker-qemu.ld`
- `tools/linker-k3.ld`
- `tools/k3.its`
- `tests/testbin-second-stage/bash`
- `tests/testbin-second-stage/busybox`
- `tests/testbin-second-stage/libc.so`

## 未包含

- `.git/`
- `target/`、`out-qemu/`、`out-k3/`
- `docs/`、`assert/`
- `.github/`、`.vscode/`
- Alien 自带的 `Makefile`
- 其他板型的 DTB 与 ITS
- 完整测试集；只保留当前构建脚本使用的三个文件

## 本地改动

本目录不是上游原样快照。K3 平台、QEMU 基线修复与构建调整都直接提交在 k3 仓库的
`alien/` 中，由 Git 历史维护，不另存补丁文件。

查看本地改动：

```bash
git log -- alien/
git diff 312f0b9..HEAD -- alien/
```

## 更新方式

上游更新时：

1. 在仓库外克隆 Alien 到新目录，检出目标 commit。
2. 生成从 `181b263` 到新 commit、且只涉及已包含路径的 diff。
3. 在 `alien/` 中应用 diff。
4. 将本地 K3 改动重新应用到新版本；可用 `git diff 312f0b9..HEAD -- alien/` 查看。
5. 更新本文件的 commit 与日期。
6. 重新运行 QEMU 与 K3 构建验证。

不要在本目录执行 `git subtree pull` 或 `git submodule`；本目录不是嵌套仓库。
