# AlienOS QEMU 基线

日期：2026-11-09
范围：Alien 内核在 QEMU `virt` 上的构建、启动与验收
不涉及：K3 开发板、K3 平台代码、磁盘与网络驱动

## 1. 固定版本

| 项目 | 值 |
|---|---|
| Alien 仓库 | `https://github.com/Godones/Alien` |
| Alien 提交 | `181b263b` |
| Rust 工具链 | `nightly-2025-05-20` |
| Rust 目标 | `riscv64gc-unknown-none-elf` |
| 内核特性 | `qemu,slab,fat` |
| QEMU | `qemu-system-riscv64` 11.1.2 |
| OpenSBI | v1.8.1，QEMU 自带 |
| 内存 | 1024 MiB |
| 处理器数量 | 1 |

工具链由 k3 仓库的 `flake.nix` 通过 rust-overlay 提供，组件与 Alien 的 `rust-toolchain.toml` 一致：

```toml
targets = [
  "riscv64gc-unknown-none-elf",
  "riscv64gc-unknown-linux-musl",
]
extensions = [
  "rust-src",
  "llvm-tools",
  "rustfmt",
  "clippy",
]
```

## 2. 补丁

### 2.1 `0001-platform-detect-plic-node.patch`

Alien 只按节点名 `plic` 查找 PLIC。QEMU 的设备树节点名是 `interrupt-controller@c000000`，因此 `MachineInfo.plic` 被解析为 `0x0..0x0`。后续写 PLIC 阈值与使能寄存器时，实际写入了地址 0 附近，内核在 `register_device_to_plic` 中挂起。

补丁同时匹配 `interrupt-controller@`，并改用 `if let Some(reg)`，避免没有 `reg` 属性的节点触发 `unwrap`。VisionFive2 的 `plic@...` 仍按原路径处理。

修改后，QEMU 中的解析结果为：

```text
PLIC: 0xc000000..0xc600000
```

### 2.2 `0002-kernel-skip-missing-kallsyms.patch`

`ksym::init_kallsyms()` 原先无条件读取 `/tests/kallsyms`。文件不存在时 `read_all` 返回空缓冲区，随后 `KallsymsMapped::from_blob` 失败并 panic。

补丁在读取失败时打印提示并返回，不中断启动：

```text
kallsyms not found, skip symbol mapping
```

需要符号回溯时，仍可用 `gen_ksym` 生成 `/tests/kallsyms`。该工具不是基线构建的必要依赖。

## 3. 构建与运行

### 3.1 源码位置与补丁

Alien 源码已放入仓库根目录的 `alien/`，三个补丁已经包含在源码中。日常构建不再需要克隆或应用补丁。

需要从上游重建参考树时，才使用：

```bash
nix develop
./scripts/alien/apply-patches.sh /tmp/opencode/alien-reference
```

脚本会克隆 Alien、检出提交 `181b263` 并应用 `docs/agents/alien/patches/` 中的补丁。

### 3.2 构建

```bash
./scripts/alien/build-qemu.sh
```

构建脚本执行以下步骤：

1. 构建 `kernel` 静态库。
2. 使用 `rust-lld` 与 `tools/linker-qemu.ld` 链接 `kernel-qemu`。
3. 构建用户态 `init`。
4. 准备 FAT 磁盘文件与 initramfs。
5. 创建 72 MiB FAT32 磁盘镜像。
6. 输出到 `$ALIEN_DIR/out-qemu`。

不使用 Alien 自带 Makefile 的原因：

- `make sdcard` 依赖 `sudo mount`。
- `make install` 会安装 `gen_ksym`。
- `make initramfs` 需要交互式配置 BusyBox。
- `make compile` 默认调用 `riscv64-linux-gnu-ld`，当前环境使用 `rust-lld`。

### 3.3 运行

```bash
./scripts/alien/run-qemu.sh
```

默认 60 秒后停止。需要交互时：

```bash
QEMU_TIMEOUT=0 ./scripts/alien/run-qemu.sh
```

## 4. 验收结果

阶段 0 当时在干净克隆上执行构建与启动；现在使用仓库内 `alien/` 可直接复现。结果为：

```text
[0] Init filesystem success
[0] kallsyms not found, skip symbol mapping
[0] Init task success
[0] kprobe test passed
[0] Begin run task...
[0] kthread_init start...
Init process is running
Alien:/#
```

启动脚本在 30 秒超时后退出，退出码为 0。去掉 ANSI 控制符后，日志中出现 `Alien:/#`。

关键日志节选见 `logs/2026-11-09-qemu-baseline.txt`。

日志中的提示符带有 ANSI 颜色控制符，直接检索 `Alien:/#` 不会命中。脚本与文档统一使用 `Alien.*#` 检索。

## 5. 已知限制

- 该基线只验证 QEMU。K3 的内存布局、SBI 实现、启动协议与外设均未验证。
- QEMU 的 `-drive file=fat:rw:<目录>` 方式不可用。Alien 的 FAT 驱动读到无效的 BPB，报 `invalid bytes_per_sector`。必须使用真实的 FAT32 镜像。
- 内核仍需要 `/dev/sda` 提供 `/tests/init`；QEMU 基线没有把 init 放进 initramfs。
- QEMU 基线的 initramfs 只包含 `/bin`，`/tests/init` 来自 FAT 磁盘。
- `init` 固定执行 `/tests/bash`，磁盘中必须提供静态 bash。
- 当前 `initramfs.cpio.gz` 约 770 KiB，FAT 镜像固定 72 MiB。
- `gen_ksym` 不存在时跳过符号表，panic 回溯只有地址。

## 6. 复现命令

```bash
cd /home/starryreverie/userdata/development/k3
nix develop
./scripts/alien/build-qemu.sh
QEMU_TIMEOUT=30 ./scripts/alien/run-qemu.sh > /tmp/opencode/alien-qemu.log 2>&1
grep -aE 'Alien.*#|Init process is running' /tmp/opencode/alien-qemu.log
```
