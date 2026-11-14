# K3 平台 QEMU 回归

日期：2026-11-09
范围：在 QEMU 上运行 K3 平台代码，验证 SBI 控制台、内存配置与 initramfs
不涉及：开发板实测、交互 shell、PLIC、UART 驱动

## 1. 目标

阶段 1 只完成了 K3 镜像构建，没有运行过 K3 平台代码。本阶段增加 `k3_qemu` 变体，在 QEMU 上复用同一套 K3 平台逻辑：

```text
K3 平台模块 + SBI DBCN 控制台 + 内嵌 initramfs + 无磁盘
```

与真实 K3 的差异只在内存范围、链接地址与时间基准频率。

## 2. 实现

### 2.1 新 feature

```toml
k3_qemu = ["platform/k3_riscv", "platform/k3_qemu", "interrupt/k3", "mem/initrd"]
```

平台 crate 增加 `k3_qemu` feature。

### 2.2 条件配置

`k3_riscv` 平台模块根据 feature 选择：

| 项目 | K3 | K3 QEMU |
|---|---|---|
| 内存 | `0x102000000..0x300000000` | `0x80000000..0xc0000000` |
| 时钟频率 | 24 MHz（阶段 3 已确认） | 10 MHz，与 QEMU 一致 |
| 链接地址 | `0x102200000` | `0x80200000` |
| PLIC | 不初始化 | 不初始化 |
| 设备探测 | 跳过 | 跳过 |

### 2.3 initramfs 与 init

`k3_qemu` 构建不包含 `/tests/bash`。用户态 `init` 增加检查：

```text
如果 /tests/bash 不存在：
    打印 K3_QEMU_INIT_OK
    循环 yield
```

这样阶段 2 只验证平台代码，不把 bash 的终端行为混入结果。

## 3. 构建与运行

```bash
nix develop
./scripts/alien/build-k3-qemu.sh
QEMU_TIMEOUT=30 ./scripts/alien/run-k3-qemu.sh
```

内核使用：

```text
tools/linker-qemu.ld
入口 0x80200000
无 virtio-blk
无外部 initrd 参数
```

## 4. 验收结果

```text
This is a device tree representation of a QEMU virt (K3 test) machine
Memory: 0x80000000..0xc0000000
PLIC:   0x0..0x0
[0] no disk filesystem feature, skip /tests disk mount
[0] Initrd populate success
[0] Init filesystem success
[0] kallsyms not found, skip symbol mapping
Init process is running
K3_QEMU_INIT_OK
```

日志中 0x01 控制字符数量为 0。

这说明：

- K3 平台模块可以在 QEMU 上初始化。
- SBI DBCN 输出可用。
- 内嵌 initramfs 可以解出 `/tests/init`。
- 没有磁盘时代码不会挂载 `/dev/sda`。
- 用户态 init 可以运行。

## 5. 已知问题

### 5.1 bash 与 SBI 控制台

阶段 2 在 initramfs 包含 `/tests/bash` 时观察到连续 `0x01`。阶段 4 已定位并修复：

- 根因在 SBI 控制台输入轮询，不在 bash 输出路径。
- 用户态 `/dev/tty` 改为轮询 UART。
- 完整镜像已在 QEMU 与真实 K3 上运行 bash。

详细记录见 `07-uart-console.md`。

### 5.2 其他限制

- 阶段 3 已确认真实 K3 的时间基准为 24 MHz、内存范围为 `0x102000000..0x300000000`。
- 未初始化 PLIC、APLIC 或 IMSIC。
- 未启用 SMP。
- 该变体只用于回归，不是最终 K3 启动路径。

## 6. 下一步

1. 阶段 3 已完成首次上板，见 `06-k3-board-boot.md`。
2. 阶段 4 已完成控制台交互修复，见 `07-uart-console.md`。
3. 之后确认 IMSIC、APLIC、UART 中断与 SMP 的接入方式。
