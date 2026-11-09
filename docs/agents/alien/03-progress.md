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
- 在干净克隆 `/tmp/opencode/Alien-clean` 上重新应用补丁、构建并启动 QEMU。
- 验收输出出现 `Alien:/#`，日志中同时出现 `kallsyms not found, skip symbol mapping`，说明可选符号表路径有效。

验收命令：

```bash
nix develop
./scripts/alien/apply-patches.sh /tmp/opencode/Alien-clean
ALIEN_DIR=/tmp/opencode/Alien-clean ./scripts/alien/build-qemu.sh
ALIEN_DIR=/tmp/opencode/Alien-clean QEMU_TIMEOUT=30 \
  ./scripts/alien/run-qemu.sh > /tmp/opencode/alien-qemu.log 2>&1
grep -aE 'Alien.*#|Init process is running' /tmp/opencode/alien-qemu.log
```

### 阶段 1：K3 最小平台

状态：未开始。

计划内容：

- 新增 `k3` feature 与平台模块。
- 控制台、定时器、关机改用 SBI DBCN、TIME、SRST。
- 内存范围固定为 `0x102000000..0x300000000`。
- 链接地址 `0x102200000`，新增 `tools/linker-k3.ld`。
- 跳过 PLIC、设备探测与 FAT 磁盘挂载。
- 使用 initramfs 提供 `/tests/init` 与静态 bash。
- 生成 FIT 镜像，准备 `bootm` 启动命令。

进入阶段 1 前需要确认：

1. 第一版验收目标是启动并运行 `/tests/init`，还是必须到可交互 shell。
2. Alien 的 K3 改动放在独立分支与补丁集，还是继续在临时克隆上修改。
3. 是否允许使用主线 K3 设备树作为参考，暂时不提取板端 vendor DTB。

### 待确认

- K3 的 `timebase-frequency`。
- K3 使用 PLIC、APLIC 还是 IMSIC。
- K3 UART 的基址、兼容字符串与中断号。
- 开发板 `/boot` 或 `/sys/firmware/fdt` 是否提供可用的 vendor DTB。
