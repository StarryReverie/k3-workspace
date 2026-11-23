# QEMU 回归矩阵

适用硬件：QEMU `virt`，riscv64，1/2/4/8 个 hart
适用系统：AlienOS `isolation`，提交见 [`../README.md`](../README.md)
适用工具：`nightly-2024-10-25`、QEMU 9 及以上
记录时间：2026-11-22
相关文档：[`../setup/qemu-baseline.md`](../setup/qemu-baseline.md)、[`../plan/roadmap.md`](../plan/roadmap.md)

本文记录 M1 的 QEMU 回归骨架：按核数重复构建与启动、从 FAT 磁盘动态装载域、
以及让串口输出可以稳定断言。本文不涉及 K3 平台、PCIe 与 NVMe。

## 场景

测试入口为 `scripts/alien-isolation/test-qemu.py`。

| 场景 | 核数 | 验证内容 |
|---|---|---|
| `baseline` | 2 | 启动到 shell，输出 `M1_OK` |
| `smp-1` | 1 | 只有启动 hart，输出 `M1_SMP1_OK` |
| `smp-2` | 2 | 1 个从 hart，输出 `M1_SMP2_OK` |
| `smp-4` | 4 | 3 个从 hart，输出 `M1_SMP4_OK` |
| `smp-8` | 8 | 7 个从 hart，输出 `M1_SMP8_OK` |
| `disk-load` | 2 | 用 `dvfs` 从 FAT 盘装载 `gvfs2`，替换运行中的 `vfs` 域，输出 `M1_DISK_OK` |
| `blk-crash` | 2 | 块域带故障注入启动，验证 `DOMAINCRASH` 捕获与重读，输出 `M6_CRASH_OK` |
| `block-swap` | 2 | 拒绝不支持的替换，再把 `shadow_blk-1` 换成 `sshadow_blk` 并读文件，输出 `M6_SWAP_OK` |

`all` 依次执行 `smp-1`、`smp-2`、`smp-4`、`smp-8`、`disk-load`、`blk-crash`
与 `block-swap`。后两个场景的机制见
[`domain-recovery.md`](domain-recovery.md)。

## 构建

```bash
nix develop

# 单个核数
./scripts/alien-isolation/build-qemu.sh --smp 8 --log WARN

# 查看场景
./scripts/alien-isolation/test-qemu.py --list

# 构建并运行全部场景
./scripts/alien-isolation/test-qemu.py --build all
```

构建脚本按核数使用独立目录：

```text
alien-isolation/build/smp-1/
alien-isolation/build/smp-2/
alien-isolation/build/smp-4/
alien-isolation/build/smp-8/
```

每个目录包含 `kernel`、`initramfs.cpio.gz`、`sdcard.img` 与 `build-info`。
`build-info` 记录 `SMP`、`LOG`、域数量和用户程序列表。

## 域产物与核数解耦

内核的 `CPU_NUM` 由 `SMP` 环境变量决定，必须与 QEMU 的 hart 数一致。域侧只有
`plic` 与 `ksync` 按 hart 编号索引数组，两者改用固定的 `MAX_CPU_NUM = 8` 后，
域 ELF 不再依赖具体核数。

因此矩阵构建只需重建内核。首次构建某一日志级别时域会完整编译一次，之后
`--smp 1`、`--smp 4`、`--smp 8` 各约 10 秒。

## FAT 布局

`sdcard.img` 挂载为 `/tests`，包含：

```text
/init            用户态 init
/bash            完整 bash
/dvfs            装载 vfs2 的用户程序
/dlog            装载 xlogger 的用户程序
/dtest           替换 shadow_blk 为 sshadow_blk 的用户程序
/dtask           装载 random_scheduler 的用户程序
/dnet            装载 virtio_mmio_net 的用户程序
/hello.txt       块设备读取验证用的文本文件
/g*              8 个 disk 域
/domains/g*      同一批 disk 域
```

`/tests/g*` 与 `/tests/domains/g*` 同时保留，因为上游工具使用两种路径。

## 断言

测试按以下顺序等待：

1. `exec args: ["sh`，表示用户态 init 已执行 `/bin/sh`。
2. `Boot hart N` 与其余每个 hart 的 `hart N start...`。
3. shell 往返探测：反复发送 `echo __M1_READY__`，直到该标记独占一行。
4. 场景标记，且标记必须独占一行。
5. 串口输出不得包含 `Kernel panic` 或 `panicked at`。

用户态 init 的 `Init process is running` 走 VFS 输出路径，8 核下偶发丢失，
即使 shell 已经可用。因此测试不把它作为前置条件，改用可重试的 shell 往返
确认输入输出链路。

启动 hart 由 QEMU 决定。4 核与 8 核日志中启动 hart 为 3，因此测试从
`Boot hart` 与实际 hart 编号计算应出现的从 hart 集合，不假设启动 hart 为 0。

`disk-load` 在等待 shell 后发送 `dvfs`，并要求出现：

```text
<sys_load_domain> domain_name: "vfs2", ty:VfsDomain, size: 445KB
Try to replace vfs domain vfs with vfs2 ok
Test register and update vfs domain success
```

随后发送 `echo M1_DISK_OK`，确认替换后的 `vfs2` 仍能服务 shell。

## 日志稳定性

M1 做了三处修改。

- 内核构建不再传入 `LOG`。内核调度器的 `yield_now` 与 `<add_task>` 是 `WARN`
  级日志，开启后会在每个调度点输出，反而掩盖启动信息。默认保持关闭。
- 域使用 `--log WARN` 构建，去掉 syscall 域逐次系统调用的 `INFO` 输出。
- 域打印先在域内格式化整行，再通过一次 `write_console` 写入。内核为每次
  写控制台持锁，因此一行不会被其它 hart 的输出拆散。

效果：2 核日志从约 5.7 MB 降到约 24 KB，完整启动可在数秒内完成断言。

日志级别变化会触发重新编译。`basic` 与 `platform` 的构建脚本声明了
`rerun-if-env-changed=LOG`，`config` 声明了 `rerun-if-env-changed=SMP` 与
`rerun-if-env-changed=PLATFORM`。

## 验收结果

2026-11-22 执行：

```bash
./scripts/alien-isolation/test-qemu.py --no-build baseline smp-1 smp-2 smp-4 smp-8 disk-load
```

结果：

| 场景 | 启动 hart | 从 hart | 日志大小 | 结果 |
|---|---|---|---|---|
| `baseline` | 0 | 1 | 24 KB | 通过 |
| `smp-1` | 0 | 无 | 24 KB | 通过 |
| `smp-2` | 0 | 1 | 24 KB | 通过 |
| `smp-4` | 3 | 0、1、2 | 25 KB | 通过 |
| `smp-8` | 3 | 0、1、2、4、5、6、7 | 26 KB | 通过 |
| `disk-load` | 0 | 1 | 34 KB | 通过 |

完整输出：

```text
PASS alien-isolation qemu: 6 scenarios
```

日志节选见 [`../logs/qemu-matrix.txt`](../logs/qemu-matrix.txt)。

## 执行频率

| 时机 | 场景 |
|---|---|
| 日常修改与单点调试 | `baseline` 或 `smp-2` |
| 提交前 | `all` |
| 修改 SMP、调度、中断或内存布局 | `all`，并对 `smp-8` 重复运行 |
| M2 之后上板 | K3 的 1/2/8 核，不能由 QEMU 矩阵替代 |

每个核数是独立的内核二进制，`CPU_NUM` 参与启动栈、per-CPU 数组与 PLIC 上下文。
启动 hart 也随核数变化：实测 1/2 核为 0，4/8 核为 3。因此只测 2 核会漏掉
启动 hart 非 0 的路径。

现有场景只验证 hart 启动与 shell 可用，不施加并行负载。多核竞态、锁竞争与
中断并发需要另行增加压力场景。

## 已知限制

- `MAX_CPU_NUM` 固定为 8。超过 8 个 hart 需要同时调整该常量与数组布局。
- 动态装载只验证 `vfs` 替换为 `vfs2`。`dlog`、`dtask`、`dnet`、`dtest`
  已随镜像提供，但尚未纳入自动断言。
- 测试只覆盖 QEMU `virt`，不验证 K3 的 SBI、AIA 与 SDHCI。
- 串口日志仍包含 `WARN` 级的 ELF 装载与信号设置信息，属于预期输出。
