# 日志节选

本目录保存关键日志节选，用于复核构建与启动结果。

- 文件按日期命名。
- 只保留能够说明问题的行，不保存完整 ANSI 输出。
- 完整日志仍保留在临时目录，不提交。

当前文件：

- `2026-11-09-qemu-baseline.txt`：Alien QEMU 基线启动日志。
- `2026-11-09-k3-build.txt`：K3 最小镜像的构建与静态检查节选。
- `2026-11-09-k3-qemu.txt`：K3 平台代码的 QEMU 回归日志。
- `2026-11-10-k3-high-qemu.txt`：真实 K3 ELF 在 10 GiB QEMU 上的高地址回归。
- `2026-11-10-k3-board-boot.txt`：K3 诊断镜像的首次上板启动日志。
- `2026-11-14-k3-uart.txt`：K3 轮询 UART 输入回显测试。
- `2026-11-14-k3-bash.txt`：K3 完整 bash 镜像的 `echo` 与 `ls`。
- `2026-11-14-k3-qemu-bash.txt`：QEMU `k3_qemu` 完整 bash 交互回归。
- `2026-11-14-k3-qemu-smp.txt`：QEMU `k3_qemu -smp 2` 双核回归。
- `2026-11-14-k3-smp.txt`：K3 双核 gzip FIT 上板日志。
- `2026-11-15-k3-qemu-4core.txt`：QEMU `k3_qemu -smp 4` 四核回归。
- `2026-11-15-k3-qemu-8core.txt`：QEMU `k3_qemu -smp 8` 八核回归。
- `2026-11-15-k3-4core.txt`：K3 四核上板日志。
- `2026-11-15-k3-8core.txt`：K3 八核上板日志。
- `2026-11-15-k3-qemu-aia.txt`：QEMU AIA 中断 UART 与 `/proc/interrupts`。
- `2026-11-15-k3-aia.txt`：K3 AIA 初始化与轮询控制台。
- `2026-11-15-k3-uart-dt.txt`：K3 U-Boot UART 设备树与寄存器查询。
