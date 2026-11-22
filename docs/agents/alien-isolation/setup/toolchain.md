# 工具链

日期：2026-11-22
状态：完成
范围：alien-isolation 的 devShell 工具链选择与旧树兼容性

## 选择

全仓库只保留一套 devShell 工具链：

```text
nightly-2025-01-01
rustc 1.85.0-nightly
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

旧 `alien/` 不再保证在新 devShell 下构建，只作为宏内核版本参考。

## 为什么不是上游的 2024-10-25

`alien-isolation/rust-toolchain.toml` 记录的是 `nightly-2024-10-25`。当前
rust-overlay 已不再暴露该 nightly 的清单；使用仍包含该清单的旧 overlay 时，
`rust-src` 组件无法被 Nix 正确解包，`-Z build-std` 不能工作。

`nightly-2025-01-01` 是当前 rust-overlay 中仍然带可用 `rust-src` 的最早版本，
与上游版本最接近，因此作为统一工具链。

## 为什么不是原来的 2025-05-20

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

- `nightly-2025-01-01`：可以编译。
- `nightly-2025-05-20`：报错，要求 `#[unsafe(naked)]`。

因此不采用“保留旧工具链”的方案，也不维护两套 devShell。

## 影响

- 根目录 `flake.nix` 与 `flake.lock` 一起固定工具链。
- 上游 `rust-toolchain.toml` 保留原值，只作版本记录；实际构建以 devShell
  中的 `rustc`、`cargo` 为准。
- 旧 `alien/` 的构建脚本与测试不再作为主线验证项。
