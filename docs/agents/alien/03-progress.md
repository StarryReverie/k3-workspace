# AlienOS 阶段进度

## 2026-11-09

### 阶段 0：QEMU 基线

状态：完成。

完成内容：

- 固定 Alien 提交 `181b263b` 与工具链 `nightly-2025-05-20`。
- 清理 `init_rtc` 中的临时调试输出。
- 保留两处必要修复并导出为补丁：
  - QEMU PLIC 节点识别。
  - kallsyms 缺失时跳过符号映射。
- 新增构建与运行脚本：
  - `scripts/alien/apply-patches.sh`
  - `scripts/alien/build-qemu.sh`
  - `scripts/alien/run-qemu.sh`
- k3 仓库 devShell 增加 Alien 构建所需工具：`cpio`、`dosfstools`、`gzip`、`mtools`、`qemu`。
- 在干净克隆上重新应用补丁、构建并启动 QEMU；临时克隆在验证后删除。
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
- 新增补丁 `0003-k3-minimal-platform.patch`。

验收结果：

```text
kernel-k3         9.4 MiB
alien-k3.bin      5.0 MiB
alien-k3.itb      5.0 MiB
Entry point       0x102200000
Load address      0x102200000
```

干净克隆应用三个补丁后构建成功。QEMU 基线回归通过，输出仍出现 `Alien:/#`。

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

状态：未开始。

计划建立 `k3-qemu` 变体，复用 K3 平台代码，但使用 QEMU 的内存范围与链接地址，在 QEMU 上验证：

- SBI DBCN 输出。
- SBI 控制台 `/dev/tty`。
- initramfs 解出 `/tests/init` 与 bash。
- 无磁盘、无 PLIC 的启动路径。

### 待确认

- K3 的 `timebase-frequency`。当前使用暂定值 24 MHz。
- K3 使用 PLIC、APLIC 还是 IMSIC。
- K3 UART 的基址、兼容字符串与中断号。
- 开发板 `/boot` 或 `/sys/firmware/fdt` 是否提供可用的 vendor DTB。
- U-Boot `bootm` 在 FIT 不含 FDT 时是否会传入可用的控制设备树。
