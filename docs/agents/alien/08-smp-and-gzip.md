# K3 双核启动与 gzip FIT

日期：2026-11-14
范围：FIT gzip 压缩、K3 双核启动、SBI HSM 状态探测、启动 hart 固定
不涉及：A100 核、4/8 核扩展、UART 中断、存储与网络

## 1. 目标

阶段 4 已完成单核完整 bash。本阶段完成：

1. FIT gzip 压缩，减少上板镜像体积。
2. K3 双核启动，验证 SBI HSM 与次 hart 初始化。
3. 修复主 hart 选择竞态，保证 `Boot hart 0`。
4. 在 QEMU 与真实 K3 上验证双核 bash 交互。

## 2. FIT gzip

### 2.1 修改

`tools/k3.its`：

```text
data = /incbin/("../alien-k3.bin.gz");
compression = "gzip";
```

`build-k3.sh` 在生成 raw bin 后执行：

```bash
gzip -9 -c alien-k3.bin > alien-k3.bin.gz
```

### 2.2 结果

```text
alien-k3.bin       5.0 MiB
alien-k3.bin.gz    4.0 MiB
FIT               3.98 MiB
Compression       gzip
Load / Entry      0x102200000
```

压缩率约 20%。内核镜像中的 initramfs 已经 gzip，剩余可压缩部分主要是内核代码与只读数据。

U-Boot `bootm` 能正常解压并跳转，真实 K3 已验证。

## 3. 双核启动

### 3.1 CPU 数量

`config/build.rs` 会读取环境变量 `SMP` 并重写 `CPU_NUM`：

```text
SMP 未设置或为 1  -> CPU_NUM=1
SMP=2             -> CPU_NUM=2
```

因此构建脚本在 `K3_SMP=1` 时：

```bash
SMP=2 cargo build ... --features k3,k3_smp
```

`k3_smp` feature 只负责启用 `platform/smp`，CPU 数量由 `SMP` 环境变量控制。

`config/build.rs` 增加：

```text
cargo:rerun-if-env-changed=SMP
```

### 3.2 HSM 状态探测

`common_riscv/sbi.rs` 增加 `hart_get_status`：

```text
0 = STARTED
1 = STOPPED
2 = START_PENDING
3 = STOP_PENDING
```

`init_other_hart` 在启动前查询状态：

- 查询失败：记录并跳过。
- 不是 STOPPED：记录并跳过。
- `hart_start` 失败：记录，不 panic。

这样 A100 或固件不允许的 hart 不会直接导致启动失败。

### 3.3 主 hart 固定

早期实现使用 `STARTED.compare_exchange` 决定主 hart。次 hart 可能先进入 `main`，导致 `Boot hart 1`。

修复：

- `platform_init` 记录启动 hart：

```rust
static BOOT_HART: Once<usize> = Once::new();
pub fn boot_hart() -> usize { *BOOT_HART.get().unwrap() }
```

- 主 hart 完成初始化后置位 `INIT_DONE`。
- 次 hart 等待 `INIT_DONE` 后再初始化自身。

修复后日志稳定为：

```text
init_other_hart: cpu_num=2, boot=0
[0] hart 1 start requested
[0] Boot hart 0
[1] hart 1 start
```

## 4. 验证结果

### 4.1 QEMU 双核

```text
QEMU virt, -smp 2, k3_qemu + K3_SMP=1
init_other_hart: cpu_num=2, boot=0
[0] hart 1 start requested
[0] Boot hart 0
[1] hart 1 start
Alien:/# echo K3_OK
K3_OK
```

日志中 `0x01` 数量为 0。详见 `logs/2026-11-14-k3-qemu-smp.txt`。

### 4.2 K3 双核 gzip FIT

```text
init_other_hart: cpu_num=2, boot=0
[0] hart 1 start requested
[0] Boot hart 0
[1] hart 1 start
Alien:/# echo K3_OK
K3_OK
Alien:/# /bin/busybox ls /
bin    dev    etc    proc   root   sys    tests  tmp    var
```

日志中 `0x01` 数量为 0。详见 `logs/2026-11-14-k3-smp.txt`。

### 4.3 单核回归

```text
QEMU 基线           Alien:/#
k3_qemu 诊断镜像    K3_QEMU_INIT_OK
```

## 5. 上板时间线

本次 gzip 双核镜像上板：

```text
连接 → U-Boot 提示符       约 28 秒
上传完成                   约 1 秒
网络初始化 + DHCP          约 11 秒
tftpboot + bootm           约 2 秒
Alien:/#                   0.5 秒
命令与退出                 约 4 秒
合计                       约 47 秒
```

上传速度波动较大，历史范围约 50–90 KB/s；本次网络条件较好。

## 6. 已知问题

- 双核内核日志在共享 UART 上可能交错，例如 `[1In] it++ ...`。
- 用户态输出使用 UART MMIO，内核日志使用 SBI DBCN，两个写入者未统一。
- 当前只验证 2 个 hart，4/8 核与 A100 未测试。
- `config/build.rs` 会把 `CPU_NUM` 写回 `src/lib.rs`，构建 SMP 后工作区会出现 `CPU_NUM=2`，提交前应恢复为 1。
- UART 输入仍是忙等轮询，未接入中断。
- FIT gzip 只减少约 20%，上传仍是主要等待来源。

## 7. 复现命令

```bash
# QEMU 双核完整 bash
K3_SMP=1 K3_QEMU_SHELL=1 ./scripts/alien/build-k3-qemu.sh
QEMU_SMP=2 QEMU_TIMEOUT=10 ./scripts/alien/run-k3-qemu.sh

# K3 双核 gzip 镜像
K3_SMP=1 ./scripts/alien/build-k3.sh
./scripts/alien/boot-k3-board.py \
  --image alien/out-k3/alien-k3.itb \
  --marker "Alien:/#" \
  --post-command "echo K3_OK" \
  --post-command "/bin/busybox ls /"

# 单核回归
./scripts/alien/build-k3-qemu.sh
QEMU_TIMEOUT=6 ./scripts/alien/run-k3-qemu.sh
```
