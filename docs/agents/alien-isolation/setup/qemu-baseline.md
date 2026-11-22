# QEMU 基线

日期：2026-11-22
状态：完成
范围：未修改的 AlienOS `isolation` 在 QEMU `virt` 上的构建、启动与 shell 验证

## 目标

在进入 K3 移植前，先证明 vendor 进来的隔离内核能够按上游方式启动：

```text
kernel + 26 个 init 域 + initramfs + FAT 磁盘
    -> QEMU virt
    -> VFS、task、syscall
    -> 用户态 /tests/init
    -> /bin/sh
```

本阶段不涉及 K3、PCIe 或 NVMe。

## 构建

在仓库根目录进入 devShell：

```bash
nix develop
./scripts/alien-isolation/build-qemu.sh
```

脚本执行：

1. 使用上游 `cargo domain build-all` 构建 26 个 init 域与 8 个 disk 域。
2. 校验 `domain-list.toml` 中每个域都产生了对应 ELF。
3. 构建 `kernel`，参数为 `PLATFORM=qemu_riscv`、`SMP=2`、`-Z build-std`。
4. 构建用户态 `init` 与动态装载工具。
5. 生成 initramfs 与 72 MiB FAT 磁盘。

产物位于 `alien-isolation/build/smp-2/`：

```text
kernel              596 KiB
initramfs.cpio.gz   4.0 MiB
sdcard.img          72 MiB
build-info          核数、日志级别、26 init 域、8 disk 域
domains.log         域构建日志
kernel.log
user.log
```

M1 起构建按核数分目录，默认 `SMP=2`。内核同时复制到该目录，运行脚本无需再访问
`alien-isolation/target/`。其他核数与动态装载场景见
[`../testing/qemu-matrix.md`](../testing/qemu-matrix.md)。

## 运行

```bash
./scripts/alien-isolation/run-qemu.sh
```

默认参数：

```text
-M virt -bios default
-m 2048M -smp 2
virtio-blk-device   sdcard.img
virtio-net-device   hostfwd tcp:55555 / udp:5555
```

`QEMU_TIMEOUT=0` 进入交互；默认 60 秒后停止。

## 验收

冒烟测试：

```bash
./scripts/alien-isolation/test-qemu.py --no-build
```

结果：

```text
PASS alien-isolation qemu: 1 scenarios
```

测试等待 `exec args: ["sh`，随后通过 `echo __M1_READY__` 往返确认 shell
可读写，最后发送 `echo M1_OK` 并要求标记独占一行。完整串口记录在
`logs/tests/`。

启动日志中的关键节点：

```text
<register domain>: vfs
<register domain>: task
Init qemu plic success
init uart success
virtio_blk_addr: 0x10008000-0x10009000
net stack init successes!
Initrd populate success
fatfs mount success
Init filesystem success
Init task domain success
Load domains done
Init process is running
M0_OK
```

## 构建修复

上游 QEMU 路径依赖较旧依赖版本与特定设备树命名，需要以下最小修复。

### 工具链与交叉编译器

根 `flake.nix` 固定 `nightly-2024-10-25` 与兼容的 rust-overlay 提交，并加入
`riscv64-linux-gnu-gcc`、`riscv64-linux-musl-gcc` 的 Nix 交叉工具链别名。
细节见 [`toolchain.md`](toolchain.md)。

### domain host 工具锁

`domains/` 没有上游锁文件。最新的 `clap`、`lru`、`indexmap` 会要求 Rust 1.85。
通过 `domains/Cargo.lock`、`domains/domain/Cargo.toml` 的版本约束以及
`resolver.incompatible-rust-versions = "fallback"` 固定到 Rust 1.84 可解析版本。

### yanked core2

`vfs` 与 `vfs2` 依赖 `core2 0.4.0`，该版本已被 crates.io yank。两个子 workspace
使用根 `Cargo.lock` 作为初始锁，保留该版本并补齐其余依赖。锁文件已提交。

### PLIC 节点名

QEMU `virt` 的中断控制器节点名是 `interrupt-controller@c000000`，兼容值为
`riscv,plic0`，不是 `plic`。`kernel/src/bus/fdt.rs` 的 `probe_plic` 增加了
`interrupt-controller@` 回退匹配，并排除 `aplic`。

### virtio-net

`net_stack` 域要求存在 `nic`。QEMU 启动参数必须包含 `virtio-net-device`，
否则在 PLIC、UART、块设备之后停在 `nic domain not found`。

## 已知限制

- 日志输出较多，`syscall` 域逐字符打印，串口噪音较大。
- 当前只验证 2 核与 shell 命令；完整多核、磁盘域动态加载属于 M1。
- FAT 盘已放入 8 个 disk 域，但尚未在 QEMU 中通过 `dvfs` 等命令动态装载。
- 旧 `alien/` 不再保证在新 devShell 下构建。
