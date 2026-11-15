# AlienOS 阶段进度

## 2026-11-09

### 阶段 0：QEMU 基线

状态：完成。

完成内容：

- 固定 Alien 提交 `181b263b` 与工具链 `nightly-2025-05-20`。
- 清理 `init_rtc` 中的临时调试输出。
- 保留两处必要修复：
  - QEMU PLIC 节点识别。
  - kallsyms 缺失时跳过符号映射。
- 新增构建与运行脚本：
  - `scripts/alien/build-qemu.sh`
  - `scripts/alien/run-qemu.sh`
- k3 仓库 devShell 增加 Alien 构建所需工具：`cpio`、`dosfstools`、`gzip`、`mtools`、`qemu`。
- 在干净克隆上重新构建并启动 QEMU；临时克隆在验证后删除。
- 验收输出出现 `Alien:/#`，日志中同时出现 `kallsyms not found, skip symbol mapping`，说明可选符号表路径有效。

验收命令：

```bash
nix develop
./scripts/alien/build-qemu.sh
QEMU_TIMEOUT=30 ./scripts/alien/run-qemu.sh > /tmp/opencode/alien-qemu.log 2>&1
grep -aE 'Alien.*#|Init process is running' /tmp/opencode/alien-qemu.log
```

### 阶段 1：K3 最小平台

状态：完成。

完成内容：

- 新增 `k3` feature、`k3_riscv` 平台模块与 `tools/linker-k3.ld`。
- 内存范围固定为 `0x102000000..0x300000000`，入口 `0x102200000`。
- 控制台、定时器、关机改用 SBI DBCN、TIME、SRST，并保留旧版调用作为回退。
- 跳过 PLIC 与设备探测。
- VFS 在 `/dev/sda` 不存在时跳过磁盘挂载，并支持解出 initramfs 中的任意路径。
- 无 UART 驱动时，用 SBI 控制台提供 `/dev/tty`。
- 生成内嵌 initramfs、raw bin 与 FIT。
- 新增 `scripts/alien/build-k3.sh`。

验收结果：

```text
kernel-k3         9.4 MiB
alien-k3.bin      5.0 MiB
alien-k3.itb      5.0 MiB
Entry point       0x102200000
Load address      0x102200000
```

干净克隆验证阶段 1 改动后构建成功。QEMU 基线回归通过，输出仍出现 `Alien:/#`。

详细记录见 `04-k3-minimal.md`。

### 源码 vendor

状态：完成。

- 将 Alien 必要源码复制到仓库根目录 `alien/`，不包含 `.git`。
- 上游 commit：`181b263bbcaeb3e75340f7b1a3fd224ec4757b7e`。
- 保留核心源码、K3 与 QEMU 链接脚本、FIT 源文件，以及三个构建所需测试程序。
- 不包含 `docs/`、`assert/`、完整测试集与其他板型工具。
- `alien/VENDORED.md` 记录 commit、范围和更新方式。
- 构建脚本默认使用 `alien/`，不再依赖 `/tmp`。

### 阶段 2：QEMU 回归验证 K3 平台代码

状态：完成。

完成内容：

- 新增 `k3_qemu` feature。
- K3 平台模块在 `k3_qemu` 下使用 QEMU 内存范围 `0x80000000..0xc0000000` 与 10 MHz 时间基准。
- 使用 `tools/linker-qemu.ld`，入口 `0x80200000`。
- 跳过 PLIC、设备探测与磁盘挂载。
- 新增 `scripts/alien/build-k3-qemu.sh` 与 `run-k3-qemu.sh`。
- 为 `k3_qemu` 准备不含 bash 的 initramfs，由 init 打印 `K3_QEMU_INIT_OK` 后循环。

验收结果：

```text
QEMU virt (K3 test)
Memory: 0x80000000..0xc0000000
no disk filesystem feature, skip /tests disk mount
Initrd populate success
Init filesystem success
Init process is running
K3_QEMU_INIT_OK
```

日志中 0x01 控制字符数量为 0。

已知问题：initramfs 包含 bash 时，执行 bash 后串口出现连续 0x01。阶段 2 通过不启动 bash 绕开，交互问题留到后续阶段。详细记录见 `05-k3-qemu.md`。

### 阶段 3：开发板首次启动

状态：完成。

完成内容：

- 新增高地址 QEMU 回归：使用真实 K3 ELF 与 `-m 10G`，覆盖 `0x102200000..0x300000000`。
- `build-k3.sh` 增加 `K3_DIAG=1`，生成不含 bash 的诊断镜像。
- init 检测 `/tests/k3-diag` 后打印 `K3_BOOT_OK` 并循环。
- 在 U-Boot 中采集时间基准、UART、中断控制器、内存范围与 `fdtcontroladdr`。
- 新增 `scripts/alien/boot-k3-board.py` 与 `scripts/uboot-probe.py`。
- 在 BananaPi-1 上通过 `tftpboot` 与 `bootm` 启动诊断镜像。

验收结果：

```text
Starting kernel ...

[0] Boot hart 0
[0] This is a device tree representation of a SpacemiT K3 machine
SMP:    1
Memory: 0x102000000..0x300000000
[0] Frame allocator init success
[0] Build kernel address space success
[0] Activate paging mode success
[0] Init filesystem success
[0] Begin run task...
Init process is running
K3_BOOT_OK
```

确认的硬件事实：

- `timebase-frequency` 为 `0x016e3600`，即 24 MHz。
- UART 为 `ns16550`，基址 `0xd4017000`，控制台 `uart0:115200`。
- 中断控制器为 IMSIC，APLIC 节点均处于禁用状态。
- 内存范围 `0x100000000..0x300000000`，Linux 可见 `0x102000000..0x2ffffffff`。
- FIT 不含 FDT 时，`bootm` 需要显式传入 `$fdtcontroladdr`。

回归验证：

- QEMU 基线仍输出 `Alien:/#`。
- K3 QEMU 回归仍输出 `K3_QEMU_INIT_OK`。
- 完整 K3 镜像仍构建成功，initramfs 包含 bash。

详细记录见 `06-k3-board-boot.md`。

### 阶段 4：K3 控制台交互与 `0x01` 洪泛修复

状态：完成。

定位过程：

- 为 `SbiConsoleDevice` 补齐 termios ioctl 后，提示符正常出现，但 `0x01` 洪泛仍在。
- 串口 trace 与 GDB 断点确认洪泛不经过 `write_at`、`Stdout::write_str` 或内核 SBI putchar。
- CPU 采样显示时间主要停留在 OpenSBI 的 UART 轮询与输出循环。
- 临时让 `read_at` 只自旋、不调用 SBI getchar，`0x01` 数量变为 0，确认根因在 SBI 控制台输入轮询路径。

修复内容：

- 新增 `PollingUart`，直接轮询底层 UART。
- K3 使用 `Uart8250`，基址 `0xd4017000`；QEMU 使用 `Uart16550`，基址 `0x10000000`。
- `k3_riscv/config.rs` 映射 UART MMIO，长度按页对齐。
- `kernel/src/lib.rs` 在 VFS 初始化前注册 `UART_DEVICE`。
- `UARTDevice::ioctl` 对未知命令返回错误，不再 `unwrap` 或 `unimplemented!`。
- `K3_UARTTEST=1` 增加 UART 输入回显测试；`K3_QEMU_SHELL=1` 增加 QEMU 完整 bash 回归。

验收结果：

```text
QEMU 完整 bash:
Alien:/# echo K3_OK
K3_OK
Alien:/# /bin/busybox ls /
bin dev etc proc root sys tests tmp var

K3 UART 自测:
K3_UART_READY
uuaarrtt--ookk

K3 完整 bash:
Alien:/# echo K3_OK
K3_OK
Alien:/# /bin/busybox ls /
bin dev etc proc root sys tests tmp var
```

三处日志中的 `0x01` 数量均为 0。

回归验证：

- QEMU 基线仍输出 `Alien:/#`。
- K3 QEMU 诊断镜像仍输出 `K3_QEMU_INIT_OK`。
- 完整 K3 镜像构建成功并已上板。

上板时间线：

- 2.6 MiB 诊断镜像：连接 9 秒、上传 34 秒、启动 0.4 秒，合计约 62 秒。
- 5.2 MiB 完整镜像：连接 15 秒、上传 98 秒、启动 0.5 秒，合计约 135 秒。
- 上传速度约 50–90 KB/s，瓶颈在宿主机到 ostool 服务端。

详细记录见 `07-uart-console.md`。

### 阶段 5：FIT gzip 与 K3 双核启动

状态：完成。

完成内容：

- FIT 改为 gzip 压缩：`5.0 MiB -> 4.0 MiB`，U-Boot 能正常解压。
- 增加 `k3_smp` feature，通过 `SMP=2` 设置 `CPU_NUM=2`。
- 增加 SBI HSM `hart_get_status`，启动前查询 hart 状态，不再用 `assert` 直接 panic。
- 修复主 hart 竞态：`platform_init` 记录启动 hart，次 hart 等待 `INIT_DONE`。
- QEMU `-smp 2` 与真实 K3 双核均验证通过。

验收结果：

```text
init_other_hart: cpu_num=2, boot=0
[0] hart 1 start requested
[0] Boot hart 0
[1] hart 1 start
Alien:/# echo K3_OK
K3_OK
Alien:/# /bin/busybox ls /
bin dev etc proc root sys tests tmp var
```

日志中 `0x01` 数量为 0。

关键修正：

- `config/build.rs` 通过 `SMP` 环境变量重写 `CPU_NUM`；SMP 构建使用 `SMP=2`。
- 构建 SMP 后 `config/src/lib.rs` 会变成 `CPU_NUM=2`，提交前恢复为 1。

上板时间线：

- gzip 双核镜像约 47 秒完成。
- 上传速度受网络影响，最快约 1 秒，慢时仍可能数十秒。

详细记录见 `08-smp-and-gzip.md`。

### 阶段 6：四核、八核与亲和性

状态：完成。

完成内容：

- 增加 `K3_CPUS` 参数，支持 1、2、4、8、16 个 hart；`K3_SMP=1` 保留为双核别名。
- 构建时写入 `$OUT_DIR/smp`，QEMU 运行脚本自动读取核数。
- 默认 CPU 亲和掩码改为全部 CPU，修正 `sched_getaffinity` 与 `/sys/devices/system/cpu/online`。
- `sched_setaffinity` 明确返回 `ENOSYS`。
- 修复八核暴露的调度竞态：任务上下文保存完成后才发布到调度器。
- 为等待/僵尸任务增加临时上下文，避免向已释放任务写入寄存器。
- 空闲循环改为 `wfi`。
- 内核日志与用户态 UART 输出共用 `STDOUT` 锁。

验收结果：

```text
QEMU -smp 4        4/4 harts，nproc=4，online=0-3
QEMU -smp 8        8/8 harts，nproc=8，online=0-7
K3 四核            4/4 harts，nproc=4，online=0-3
K3 八核            8/8 harts，nproc=8，online=0-7
QEMU 八核负载      CPU 时间倍率 7.97
```

四核与八核上板均执行 `echo K3_OK` 与 `/bin/busybox ls /`，`0x01` 数量为 0。

关键修正：

- 八核时任务在 `__switch` 保存上下文前被其他 hart 窃取，触发内核 `LoadPageFault`。
- 现由每 hart 的 `PENDING` 槽位在保存完成后发布任务。
- 用户态 UART 输出与内核 SBI 日志改用同一把输出锁，`Alien:/#` 不再被逐字符拆散。

详细记录见 `09-more-cores.md`。

### 已确认

- K3 的 `timebase-frequency` 为 24 MHz。
- K3 使用 IMSIC；APLIC 节点存在但被禁用。
- K3 UART 为 `ns16550`，基址 `0xd4017000`。
- 开发板 U-Boot 提供可用的控制设备树，地址由 `fdtcontroladdr` 给出。
- U-Boot `bootm` 在 FIT 不含 FDT 时需要显式传入 FDT 地址。
- `0x01` 洪泛来自 SBI 控制台输入轮询，已由轮询 UART 替代。
- K3 UART 输入、完整 bash、`echo` 与 `/bin/busybox ls` 均已验证。
- FIT gzip 可用，双核 `Boot hart 0` + `hart 1 start` 已验证。
- X100 的 4 核与 8 核均可在 QEMU 和真实 K3 上启动。
- `nproc` 与 `/sys/devices/system/cpu/online` 能正确反映 4/8 核。
- 内核日志与用户态 UART 输出已共用输出锁。

### 仍需确认

- A100（CPU8–15）的 HSM 状态与启动方式。
- 八核日志偶发单字节串口噪声，需要确认是抓取还是 UART 传输问题。
- `sched_setaffinity` 尚未实现，调度器不强制亲和性。
- `taskset -p` 需要全局 pid 到任务的查询接口。
- U-Boot DHCP 偶发失败，需要重试上板脚本。
- UART 中断驱动与 IMSIC 的接入方式。
- PLIC/APLIC/IMSIC 的外部中断号与路由。
- UART 流控、Ctrl-C 与作业控制。
- 长时间运行下的定时器与调度精度。
