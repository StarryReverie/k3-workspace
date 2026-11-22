# 源码导入

日期：2026-11-22
状态：完成
范围：AlienOS `isolation` 分支、`domains` 与 `domain-lib` 的源码快照

## 固定版本

| 项目 | 地址 | 分支 | 提交 |
|---|---|---|---|
| Alien | `https://github.com/Godones/Alien` | `isolation` | `e37137b15093c379d735619f0be18c2d9ff2a70a` |
| domains | `https://github.com/os-module/domains` | `main` | `d4d6d8ee95fce96ed856b63c2775b40d38dda67c` |
| domain-lib | `https://github.com/os-module/domain-lib` | `main` | `136007cdca82531f97a4e8da082b2ce48b839d2f` |

Alien 快照日期为 2026-01-22；两个子模块快照日期为 2025-11-19。

## 目录约定

源码放在仓库根目录的 `alien-isolation/`。旧的 `alien/` 不删除，作为宏内核
版本参考。

`domains/` 与 `domain-lib/` 已从 Git 子模块展开为普通目录，并移除各自的
`.git`。新目录不包含 `.gitmodules`，也不使用 `git submodule`。

必须保持以下同级关系：

```text
alien-isolation/
├── domain-lib/
├── domains/
├── kernel/
├── base/
├── rvfs-ref/
├── tools/
└── user/
```

`domains/` 中的 crate 大量使用 `../../domain-lib/...` 相对路径依赖，不能把
`domain-lib/` 移到别处或改成子模块。工具链选择见 [`toolchain.md`](toolchain.md)。

## 已包含

- Alien 的 `kernel/`、`base/`、`rvfs-ref/`、`user/`、`tools/`
- `Cargo.toml`、`Cargo.lock`、`Makefile`
- `rust-toolchain.toml`、`rustfmt.toml`
- `.cargo/config.toml` 与各子工作区的 Cargo 配置
- `domains/` 全部源码与 `domain-list.toml`
- `domain-lib/` 全部源码
- 运行所需的 `user/bin/` 预编译程序与用户态源码

## 未包含

- `.git/`、`.gitmodules`
- `target/`、`build/`、`diskfs/`、`initrd/` 等构建产物
- 上游 `docs/` 与 `assert/` 中的说明、论文、演示文稿和图片
- 任何由构建脚本生成的镜像、ELF 或日志

上游 `docs/` 与 `assert/` 只作为外部参考，不进入本仓库。需要查阅时直接查看
Alien 上游对应提交。

## 第三方 crate

第三方 crate 不做 vendor。版本由 `Cargo.lock` 固定，首次构建需要网络或本地
Cargo 缓存。以下依赖仍从 crates.io 或 Git 仓库获取：

- `os-module/*` 下的 `pconst`、`page-table`、`memory_addr`、`vmspace`、`pager`
- `asterinas/pod`
- `Byte-OS/timestamp`
- `repnop/fdt`
- `nbdd0121/unwinding`
- 其余 crates.io 依赖

因此本目录不是完全离线可构建的快照；这与“只 vendor Alien、domains、
domain-lib 三个源码仓库”的约定一致。

## 本地改动

本地平台适配、构建脚本与测试直接提交在 `alien-isolation/` 中，不另存补丁
文件。上游更新时：

1. 在仓库外克隆目标提交及对应子模块。
2. 按“目录约定”一节的关系替换源码。
3. 重新应用本地 K3 改动。
4. 更新本文件的提交与日期。
5. 重新执行 QEMU 与 K3 构建验证。

## 构建入口

上游 `Makefile` 保留作参考，但其 `sdcard` 目标依赖 `sudo mount`，`initrd`
目标依赖交互式 BusyBox 配置。K3 工作流不直接使用该 Makefile，而是在
`scripts/alien-isolation/` 下提供非交互构建脚本。
