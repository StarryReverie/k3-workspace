# 工具链

日期：2026-11-22
状态：完成
范围：alien-isolation 的 Rust 与交叉编译工具链

## 选择

Rust 使用上游记录的版本：

```text
nightly-2024-10-25
rustc 1.84.0-nightly
targets:
  riscv64gc-unknown-none-elf
  riscv64gc-unknown-linux-musl
components:
  rust-src
  llvm-tools
  rustfmt
  clippy
  rust-analyzer
```

当前 rust-overlay 已移除该 nightly 的清单。根目录 `flake.nix` 固定到提交
`6604534e44090c917db714faa58d47861657690c`，该版本保留 `2024-10-25` 清单，
并且修正了 `rust-src` 归档文件名推导问题。`nix develop` 已验证
`rust-src` 位于 sysroot，`-Z build-std` 可以使用。

交叉编译器来自 nixpkgs：

```text
pkgsCross.riscv64.stdenv.cc       -> riscv64-linux-gnu-gcc
pkgsCross.riscv64-musl.stdenv.cc  -> riscv64-linux-musl-gcc
```

两者在 devShell 中通过别名提供上游构建文件期望的命令名。`domains/.cargo/config.toml`
使用 `riscv64-linux-gnu-gcc`，`user/musl` 使用 `riscv64-linux-musl-cc`。

## 为什么不是 2025-05-20

`nightly-2025-05-20` 会直接导致 isolation 编译失败。isolation 的启动与 trap
代码使用旧式 `#[naked]`，该版本要求写成 `#[unsafe(naked)]`。

最小验证：

```rust
#![feature(naked_functions)]
#[naked]
extern "C" fn f() {
    unsafe { core::arch::naked_asm!("ret") }
}
```

结果：

- `nightly-2024-10-25`：可以编译。
- `nightly-2025-05-20`：报错，要求 `#[unsafe(naked)]`。

因此全仓库只保留一套 devShell 工具链，旧 `alien/` 不再作为主线验证项。

## 依赖解析约束

`domains/` 是独立 workspace，上游没有提交 `Cargo.lock`。Cargo 1.84 如果解析到
最新依赖，会选中要求 Rust 1.85 的 `clap_lex` 或 `lru`。

`domains/.cargo/config.toml` 增加：

```toml
[resolver]
incompatible-rust-versions = "fallback"
```

`domains/Cargo.lock` 固定 host 侧 `cargo domain` 工具的兼容版本。`vfs` 与 `vfs2`
各自保留锁文件，用于继续使用已被 yank 的 `core2 0.4.0`。
