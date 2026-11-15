# K3 多核扩展：四核与八核

日期：2026-11-15
范围：`K3_CPUS` 构建参数、CPU 亲和性掩码、调度器上下文发布竞态、控制台输出锁、四核与八核验证
不涉及：A100 核（CPU8–15）、UART 中断、IMSIC/APLIC 路由、存储与网络

## 1. 目标与结论

阶段 5 已验证双核。本阶段完成：

1. 将核数从固定的 2 个扩展为可配置的 1、2、4、8、16。
2. 验证 X100 的四核与八核启动。
3. 修正默认 CPU 亲和掩码，使用户态能够看到全部 CPU。
4. 修复八核下暴露的任务上下文发布竞态。
5. 统一内核日志与用户态 UART 输出的锁，避免提示符被拆散。

结论：

- QEMU `-smp 4` 与 `-smp 8` 均通过。
- 真实 K3 的四核与八核均通过，`nproc` 分别返回 4 与 8。
- 八核负载下 QEMU 的总 CPU 时间约为墙钟时间的 7.97 倍，说明 8 个 hart 确实并行执行。
- A100（CPU8–15）未启用，见第 7 节。

## 2. K3 拓扑与范围

| 项目 | 内容 |
|---|---|
| X100 | CPU0–7，两个四核簇，`l2_cache0` 与 `l2_cache1` |
| A100 | CPU8–15，两个 AI 簇 |
| 上游设备树 | `k3.dtsi` 目前只描述 `cpu@0` 到 `cpu@7` |
| 中断描述 | IMSIC、APLIC、CLINT 的 `interrupts-extended` 也只覆盖 CPU0–7 |
| 本阶段目标 | CPU0–7，即全部 X100 |

`CPU_NUM` 同时决定 `per_cpu`、`ksync`、`CPU_MANAGER`、调度器队列与内核栈数组的大小，并且 hart 编号直接作为这些数组的下标。因此当前实现要求 hart 编号连续，且 `CPU_NUM` 大于最大 hart 编号。

## 3. 构建参数

### 3.1 `K3_CPUS`

构建脚本新增 `K3_CPUS`：

```bash
K3_CPUS=1   # 单核
K3_CPUS=2   # 双核
K3_CPUS=4   # 四核
K3_CPUS=8   # 八核
K3_CPUS=16  # 预留给 A100 探测
```

旧参数保留兼容：

```bash
K3_SMP=1    # 等价于 K3_CPUS=2
```

脚本会把核数写入 `$OUT_DIR/smp`。`run-k3-qemu.sh` 在未设置 `QEMU_SMP` 时读取该文件，避免镜像核数与 QEMU 核数不一致。

### 3.2 `CPU_NUM` 与 `CPU_MASK`

`config/build.rs` 仍通过环境变量 `SMP` 重写 `CPU_NUM`。本阶段新增：

```rust
pub const CPU_MASK: usize = (1usize << CPU_NUM) - 1;
```

构建八核后 `CPU_NUM=8`，提交前恢复为 1。源码恢复不影响已经生成的镜像。

## 4. CPU 亲和性修正

### 4.1 默认掩码

原实现：

```rust
affinity.set_bits(0..CPU_NUM, 1 << CPU_NUM - 1);
```

该表达式只置最高位。例如八核时掩码为 `0x80`，`nproc` 会认为只有一个 CPU 可用。

修正后，进程、线程与内核线程的默认掩码统一为 `CPU_MASK`。八核时掩码为 `0xff`。

### 4.2 `sched_getaffinity`

- 按 `size` 复制，最多复制 `size_of::<usize>()` 字节。
- 返回实际写入的字节数。
- 支持 `pid == 0` 与当前进程 pid；其他 pid 返回 `ESRCH`。
- `mask == 0` 或 `size == 0` 返回 `EINVAL`。

### 4.3 `sched_setaffinity`

当前调度器不按亲和性过滤任务，因此该接口返回 `ENOSYS`，避免用户态误认为绑定已经生效。

若要支持 `taskset` 绑定，需要在调度器的任务选择路径中检查 `cpu_affinity`，并建立 pid 到任务的查询接口。该项留待后续。

### 4.4 `/sys` 输出

`CpuPossible::read_at` 原实现忽略 `offset`，总是从字符串开头复制。修正后 `/sys/devices/system/cpu/online` 正确返回：

```text
四核：0-3
八核：0-7
```

## 5. 八核调度竞态修复

### 5.1 现象

八核 QEMU 启动后出现内核态异常：

```text
unhandled trap: Exception(LoadPageFault), stval: 0x28, sepc: 0x80263a7c
```

`sepc` 位于 `__switch` 的 `ld ra, 0x0(a1)`，说明第二个上下文指针非法。

### 5.2 原因

任务让出 CPU 时，原实现先调用 `GLOBAL_TASK_MANAGER.add_task` 把任务放回队列，再调用 `__switch` 保存当前上下文。其他 hart 可以在上下文保存完成前窃取该任务，读取到尚未写入或不完整的 `ra`、`sp` 与 `s*` 寄存器。

双核时窗口较小，八核时空闲 hart 更多，问题稳定暴露。

### 5.3 修复

`kernel/src/task/schedule.rs` 增加每 hart 的待发布槽位：

```text
任务让出
  -> 写入 PENDING[hart]
  -> __switch 保存任务上下文并回到 idle
  -> idle 循环发布 PENDING[hart] 到调度器
```

这样任务只有在上下文完整保存后才会被其他 hart 看到。

同时，等待与僵尸任务可能在 `schedule_now` 中被释放。原实现仍会向已释放任务的上下文写入寄存器。修复后为每个 CPU 增加 `scratch` 上下文，已释放任务使用 `scratch` 作为保存目标。

`context::switch` 增加上下文指针校验，非法指针会打印 hart 与两个指针并 panic，便于后续定位。

### 5.4 空闲循环

`run_task` 无任务时由忙等改为 `wfi`。`init_trap_subsystem` 已开启全局中断，每个 hart 进入调度前也已设置定时器，因此 `wfi` 能被下一个时间片唤醒。

## 6. 控制台输出锁

八核启动时，用户态 UART 输出与内核 SBI 日志没有共同锁，`Alien:/#` 提示符可能被内核日志逐字符插入，导致自动测试无法识别标记。

修复：

- `platform::console_write` 改为获取与内核日志相同的 `STDOUT` 锁。
- 新增 `platform::console::with_console_lock`，供 UART 驱动复用同一把锁。
- `UARTDevice` 的输出与回显全部在该锁内完成。

修复后八核上板能够稳定识别 `Alien:/#`。日志中仍偶发单字节串口噪声，例如 `Init process is \xf2unning`，属于串口传输或抓取问题，不伴随 trap，也不影响命令执行。

## 7. A100 与 16 核

K3 的 A100 为 CPU8–15。上游 `k3.dtsi` 当前只描述 CPU0–7，IMSIC、APLIC 与 CLINT 的中断描述也只覆盖 CPU0–7。

本阶段未启用 A100。后续可增加探测模式：

```text
CPU_NUM=16
仅打印 hart 0–15 的 hart_get_status
不启动任何次 hart
```

根据状态决定是否单独构建 `8–15` 的 A100 镜像。跨簇混跑不作默认配置。

## 8. 验证结果

### 8.1 QEMU 四核

```text
SMP: requested 3 secondary harts, target 4
SMP: 4/4 harts online
Alien:/# /bin/busybox nproc
4
Alien:/# /bin/busybox cat /sys/devices/system/cpu/online
0-3
Alien:/# echo K3_OK
K3_OK
```

详见 `logs/2026-11-15-k3-qemu-4core.txt`。

### 8.2 QEMU 八核

```text
SMP: requested 7 secondary harts, target 8
SMP: 8/8 harts online
Alien:/# /bin/busybox nproc
8
Alien:/# /bin/busybox cat /sys/devices/system/cpu/online
0-7
```

八核负载测试：启动 8 个后台忙循环，QEMU 进程在 3 秒墙钟时间内累计约 23.92 秒 CPU 时间，倍率 7.97。

详见 `logs/2026-11-15-k3-qemu-8core.txt`。

### 8.3 K3 四核

```text
SMP: requested 3 secondary harts, target 4
SMP: 4/4 harts online
Alien:/# /bin/busybox nproc
4
Alien:/# /bin/busybox cat /sys/devices/system/cpu/online
0-3
Alien:/# echo K3_OK
K3_OK
Alien:/# /bin/busybox ls /
bin    dev    etc    proc   root   sys    tests  tmp    var
```

日志中 `0x01` 数量为 0，无 trap。详见 `logs/2026-11-15-k3-4core.txt`。

### 8.4 K3 八核

```text
SMP: requested 7 secondary harts, target 8
SMP: 8/8 harts online
Alien:/# /bin/busybox nproc
8
Alien:/# /bin/busybox cat /sys/devices/system/cpu/online
0-7
Alien:/# echo K3_OK
K3_OK
Alien:/# /bin/busybox ls /
bin    dev    etc    proc   root   sys    tests  tmp    var
```

日志中 `0x01` 数量为 0，无 trap。详见 `logs/2026-11-15-k3-8core.txt`。

### 8.5 单核回归

```text
QEMU 基线           Alien:/#
k3_qemu 诊断镜像    K3_QEMU_INIT_OK
```

单核构建下 `SMP` 相关代码通过条件编译排除，`CPU_MASK=1`。

## 9. 上板时间线

八核 gzip 镜像约 4.0 MiB：

```text
连接 → U-Boot 提示符       约 9–20 秒
上传完成                   约 7–22 秒
DHCP + tftpboot + bootm    约 10–12 秒
Alien:/#                   约 0.5 秒
```

U-Boot 网络初始化存在偶发失败：

```text
EQOS_DMA_MODE_SWR stuck
*** ERROR: `ipaddr' not set
```

失败发生在 DHCP 阶段，镜像未启动。重试后恢复正常。

## 10. 已知问题

- A100（CPU8–15）尚未探测与启用。
- `sched_setaffinity` 返回 `ENOSYS`，调度器不强制亲和性。
- `taskset -p` 需要按 pid 查询其他进程，当前没有全局 pid 表。
- 八核日志偶发单字节串口噪声，未观察到 trap。
- UART 输入仍是忙等轮询，未接入 IMSIC 中断。
- `config/build.rs` 会把 `CPU_NUM` 写回源码，提交前需恢复为 1。
- U-Boot DHCP 偶发失败，需要重试上板脚本。

## 11. 复现命令

```bash
# QEMU 四核
K3_CPUS=4 K3_QEMU_SHELL=1 ./scripts/alien/build-k3-qemu.sh
QEMU_SMP=4 QEMU_TIMEOUT=15 ./scripts/alien/run-k3-qemu.sh

# QEMU 八核
K3_CPUS=8 K3_QEMU_SHELL=1 ./scripts/alien/build-k3-qemu.sh
QEMU_SMP=8 QEMU_TIMEOUT=15 ./scripts/alien/run-k3-qemu.sh

# K3 四核镜像
K3_CPUS=4 ./scripts/alien/build-k3.sh
./scripts/alien/boot-k3-board.py \
  --image alien/out-k3/alien-k3.itb \
  --marker "Alien:/#" \
  --post-command "/bin/busybox nproc" \
  --post-command "/bin/busybox cat /sys/devices/system/cpu/online" \
  --post-command "echo K3_OK" \
  --post-command "/bin/busybox ls /"

# K3 八核镜像
K3_CPUS=8 ./scripts/alien/build-k3.sh
```

构建后恢复默认配置：

```bash
# 将 alien/subsystems/config/src/lib.rs 中的 CPU_NUM 恢复为 1
```
