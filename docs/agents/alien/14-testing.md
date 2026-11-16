# QEMU 与上板测试脚本

日期：2026-11-16
状态：QEMU 阶段完成，K3 上板阶段待补
范围：把阶段 9A 的手工 QEMU 回归固化为可重复执行的场景

## 1. 目的

阶段 9A 的验证原先依赖临时 bash 命令、固定等待与人工观察。问题是：

- 命令只存在于对话或临时目录，难以复现。
- 失败时缺少完整串口日志。
- 多核、AIA、NVMe 等组合容易漏测。
- 每次都要临时编写 PTY 驱动。

现在把 QEMU 测试拆成公共会话库与场景运行器：

```text
scripts/alien/qemu_pty.py      QEMU PTY 会话与日志
scripts/alien/test-qemu.py     场景、构建与断言
logs/tests/                    每次运行的完整日志，不提交
```

构建仍由 `build-k3-qemu.sh` 完成，运行仍由 `run-k3-qemu.sh` 完成。
测试脚本只负责组合参数、等待标记、执行命令与判断结果。

## 2. 使用方式

在仓库根目录进入 devShell 后执行：

```bash
nix develop

# 列出场景
./scripts/alien/test-qemu.py --list

# 运行单个场景并重新构建
./scripts/alien/test-qemu.py nvme-mount --build

# 使用已有产物运行
./scripts/alien/test-qemu.py nvme-mount --no-build

# 运行全部场景
./scripts/alien/test-qemu.py all

# 只运行名称匹配的场景
./scripts/alien/test-qemu.py all --filter '^(nvme|smp)'
```

脚本会检查 `qemu-system-riscv64` 是否在 `PATH` 中；缺失时提示进入
`nix develop`。

## 3. 场景

| 场景 | 构建 | QEMU 配置 | 主要断言 |
|---|---|---|---|
| `baseline` | 1 核、initramfs shell | 无磁盘 | 启动到 `Alien:/#`，标记命令返回 |
| `no-nvme` | 1 核、initramfs shell | 无磁盘 | `NVMe: no device`，系统继续启动 |
| `smp2` | 2 核、initramfs shell | `QEMU_SMP=2` | `SMP: 2/2 harts online`，`nproc=2` |
| `aia` | 2 核、initramfs shell | `QEMU_AIA=aplic-imsic` | 中断控制台正常 |
| `smp8` | 8 核、initramfs shell | `QEMU_SMP=8` | `SMP: 8/8 harts online`，`nproc=8` |
| `virtio-disk` | 1 核、FAT 磁盘 | `QEMU_DISK=1` | 挂载 `/tests`，`ls` 与 `cat` 正常 |
| `nvme-mount` | 1 核、FAT 磁盘 | `QEMU_DISK=1 QEMU_NVME=1` | NVMe 注册、挂载与文件读取 |
| `nvme-aia` | 1 核、FAT 磁盘 | AIA + NVMe | 中断控制台、NVMe 挂载与读取 |

磁盘场景使用 `build-k3-qemu.sh` 的 `K3_QEMU_DISK=1` 模式，镜像中固定包含
`HELLO.TXT`，内容为 `alien qemu nvme mount ok`。

## 4. 断言方式

每个场景按以下顺序执行：

1. 等待 `Alien:/#`，确认用户态已经运行。
2. 检查场景要求的启动日志，例如 `NVMe: registered /dev/sda`。
3. 逐条发送命令，并等待对应输出。
4. 发送 `echo __TEST_"OK"__`，等待 `__TEST_OK__`。

标记命令故意带引号，使命令行回显为 `__TEST_"OK"__`，而实际输出为
`__TEST_OK__`，避免等待到命令回显而不是执行结果。

等待使用正则与轮询，不以固定 `sleep` 作为主要同步手段。单条命令默认
超时 10 秒，启动默认超时 90 秒。

## 5. 构建与状态恢复

`test-qemu.py --build` 会根据场景选择构建参数：

```text
K3_CPUS=1|2|8
K3_QEMU_SHELL=1
K3_QEMU_DISK=1
```

构建脚本会改写 `alien/subsystems/config/src/lib.rs` 的 `CPU_NUM`。
运行器在每次构建后以及退出前恢复原始文件内容，避免把多核配置留在
工作区。

构建脚本在 `alien/out-k3-qemu/build-info` 记录 `CPUS`、`DISK` 与
`SHELL`。使用 `--no-build` 时，运行器会校验产物变体；不匹配时直接
报告并退出，不再让错误的内核进入启动流程。

同一构建变体在连续场景之间复用；变体变化时才重新构建。

## 6. 日志与结果

每次运行生成：

```text
logs/tests/<时间>-<场景>.log
logs/tests/build-<变体>.log
```

日志包含实际启动命令、完整串口输出与 PASS/FAIL 原因。终端只打印摘要：

```text
[build] cpus1-disk-plain
PASS nvme-mount (0.8s)

8 scenario(s) passed
```

失败时打印最后 20 行并给出日志路径，退出码为 1。

`logs/tests/` 已加入 `.gitignore`；需要长期保留的节选再整理到
`docs/agents/alien/logs/`。

## 7. 第一阶段结果

2026-11-16 运行 `test-qemu.py all --build`，八个场景全部通过：

```text
PASS baseline
PASS no-nvme
PASS smp2
PASS aia
PASS smp8
PASS virtio-disk
PASS nvme-mount
PASS nvme-aia
```

## 8. 后续

- 增加 `test-k3-board.py`，包装 `boot-k3-board.py`，覆盖启动、SMP、
  存储与只读 PCIe 取证。
- 上板测试默认只读；写 SD 卡或测试镜像必须显式授权。
- 增加 `high` 高地址 QEMU 场景。
- 如需要，把完整 QEMU 矩阵接入提交前检查。
