# K3 最小平台

日期：2026-11-09
范围：Alien 在 SpacemiT K3 上的第一版平台代码、镜像构建与静态验收
不涉及：开发板实测、PLIC、UART 驱动、存储与网络

## 1. 目标

第一版只验证以下链路：

```text
U-Boot -> SBI -> Alien 内核 -> 内存管理 -> initramfs -> /tests/init
```

不接磁盘、网卡与外部中断。控制台通过 SBI DBCN 输出，用户态通过内嵌 initramfs 启动。

## 2. 代码改动

### 2.1 新增 `k3` feature

```toml
k3 = ["platform/k3_riscv", "interrupt/k3", "mem/initrd"]
```

平台 crate 新增 `k3_riscv`，内核在 `k3` 下跳过：

- `interrupt::init_plic()`
- `devices::init_device()`

### 2.2 平台参数

`subsystems/platform/src/k3_riscv/config.rs`：

```text
CLOCK_FREQ  24_000_000     # 暂定值，待设备树或实测确认
HEAP_SIZE   0x200_00000    # 512 MiB
MMIO        []             # 第一版不使用外部设备
```

`subsystems/platform/src/k3_riscv/mod.rs`：

```text
内存范围    0x102000000 .. 0x300000000
内核入口    0x102200000
PLIC        未初始化
CLINT       未使用
initramfs   内嵌在 .rodata
```

保留区 `0x100000000..0x101ffffff` 不在可用内存范围内。

### 2.3 SBI 调用

`subsystems/platform/src/common_riscv/sbi.rs` 改为优先使用标准扩展：

| 功能 | 扩展 | 回退 |
|---|---|---|
| 控制台输出 | DBCN | 旧版 console |
| 控制台输入 | DBCN | 旧版 console |
| 定时器 | TIME | 旧版 set_timer |
| 关机 | SRST | 旧版 shutdown |
| 多核启动 | HSM | 无 |

K3 的 OpenSBI 明确报告 TIME、SRST、DBCN 与 HSM，因此第一版不依赖旧版调用。

### 2.4 initramfs

`subsystems/vfs/src/initrd.rs` 改为解出 cpio 中的全部路径，不再只处理 `bin/` 与 `sbin/`。第一版 initramfs 包含：

```text
bin/busybox
bin/sh -> busybox
tests/init
tests/bash
tests/libc.so
```

`/tests` 不再要求块设备。`subsystems/vfs/src/lib.rs` 在 `/dev/sda` 不存在时跳过磁盘挂载，只保留 ramfs 中的 `/tests`。

### 2.5 SBI 控制台设备

没有 UART 驱动时，`/dev/tty` 由新增的 `SbiConsoleDevice` 提供：

- 写入调用 `platform::console::console_write`。
- 读取轮询 `platform::console_getchar`。
- 该设备只作为第一版的兜底，不实现完整 termios 与行编辑。

### 2.6 链接与 FIT

新增：

```text
tools/linker-k3.ld     BASE_ADDRESS = 0x102200000
tools/k3.its           FIT，load/entry 为 0x102200000
```

FIT 不包含 DTB 与 ramdisk。内核内嵌 initramfs，第一版不解析设备树。

## 3. 构建

```bash
nix develop
./scripts/alien/build-k3.sh
```

构建顺序与 QEMU 基线不同：

1. 构建用户态 `init`。
2. 生成 initramfs。
3. 构建内核，`include_bytes!` 嵌入 initramfs。
4. 链接、生成 raw bin、生成 FIT。

`include_bytes!` 不一定会因被包含文件变化而触发重建，脚本会 `touch` K3 平台模块。

## 4. 产物

```text
out-k3/kernel-k3       9.4 MiB   含调试信息
alien-k3.bin           5.0 MiB   raw binary
out-k3/alien-k3.itb    5.0 MiB   FIT
```

FIT 信息：

```text
Load Address: 0x102200000
Entry Point:  0x102200000
Architecture: RISC-V
OS:           Linux
Compression:  uncompressed
```

## 5. 静态验收

```text
ELF Entry point address: 0x102200000
raw bin 首条指令:          mv tp, a0
initramfs:                bin/busybox, bin/sh, tests/init, tests/bash, tests/libc.so
FIT Data Size:            5201920 Bytes
```

`alien-k3.bin` 的首字节与 `_start` 一致，说明 `objcopy -O binary` 没有在文件头写入 ELF 头或额外填充。

## 6. QEMU 回归

K3 改动合入后，重新构建并启动 QEMU 基线：

```text
[0] mount fs success
[0] kallsyms not found, skip symbol mapping
Init process is running
Alien:/#
```

说明 SBI 标准扩展、VFS 改动与 initramfs 解析没有破坏已有平台。

## 7. 干净克隆验证

阶段 1 当时从 `181b263` 克隆，依次应用：

```text
0001-platform-detect-plic-node.patch
0002-kernel-skip-missing-kallsyms.patch
0003-k3-minimal-platform.patch
```

构建成功，FIT 的加载地址与入口均为 `0x102200000`。

这三个补丁现在已包含在仓库根目录的 `alien/` 中；日常构建直接使用该目录。

## 8. 已知限制

- 未在开发板上启动。
- `CLOCK_FREQ` 使用暂定的 24 MHz，时间与调度 tick 可能不准。
- 不解析设备树，UART、PLIC、APIC、IMSIC 的地址与中断号均未确认。
- 未初始化 PLIC，外部中断不可用。
- `/dev/tty` 的读取是忙等待轮询，不处理 termios、回显与行编辑。
- 未启用 SMP，只使用启动 hart。
- 未接入 UFS、SD、网卡。
- FIT 不含 DTB。若 U-Boot 的 `bootm` 要求 FDT，需要补充设备树或确认控制 FDT 可用。
- 内核堆固定 512 MiB，后续可根据实际内存调整。

## 9. 下一步

1. 建立 `k3-qemu` 变体，在 QEMU 上运行 K3 平台代码，验证 SBI 控制台与 initramfs。
2. 确认 K3 的 `timebase-frequency` 与中断控制器类型。
3. 上板使用 U-Boot `tftpboot` 与 `bootm` 启动 FIT。
