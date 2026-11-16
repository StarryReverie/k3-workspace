# QEMU 与上板测试脚本

日期：2026-11-16
状态：QEMU、K3 默认上板场景与构建检查完成；PCIe 取证待授权
范围：把手工回归固化为可重复执行的场景、断言与日志

## 1. 结构

测试拆成三部分，避免所有逻辑堆在一个脚本里：

```text
scripts/alien/qemu_pty.py        QEMU PTY 会话与日志
scripts/alien/test-qemu.py       QEMU 运行时场景
scripts/alien/test-k3-board.py   K3 上板场景
scripts/alien/test-build.py      构建、ELF 与 FIT 检查
logs/tests/                      每次运行的完整日志，不提交
```

构建与运行仍由现有脚本完成：

```text
build-qemu.sh / run-qemu.sh
build-k3-qemu.sh / run-k3-qemu.sh
build-k3.sh / boot-k3-board.py
run-k3-high-qemu.sh
```

测试脚本只负责组合参数、等待标记、执行命令、检查产物与汇总结果。

## 2. 使用方式

在仓库根目录进入 devShell 后执行：

```bash
nix develop

# QEMU
./scripts/alien/test-qemu.py --list
./scripts/alien/test-qemu.py nvme-mount --build
./scripts/alien/test-qemu.py all
./scripts/alien/test-qemu.py all --filter '^(nvme|smp)'

# K3 上板
./scripts/alien/test-k3-board.py --list
./scripts/alien/test-k3-board.py boot --build
./scripts/alien/test-k3-board.py smp8 --build
./scripts/alien/test-k3-board.py all
./scripts/alien/test-k3-board.py pcie-recon --dry-run

# 构建与产物
./scripts/alien/test-build.py --list
./scripts/alien/test-build.py all
```

`test-qemu.py` 会检查 `qemu-system-riscv64` 是否在 `PATH` 中；缺失时提示
进入 `nix develop`。`test-k3-board.py` 需要可用的板卡租约与 `ostool`
凭据。

## 3. QEMU 场景

| 场景 | 构建 | QEMU 配置 | 主要断言 |
|---|---|---|---|
| `qemu-baseline` | 上游 `qemu` 特性 | virtio 磁盘 | 挂载 `/tests`，`ls` 正常 |
| `high` | K3 诊断镜像 | 10 GiB、K3 链接地址 | 高地址内存、页表、帧分配器 |
| `baseline` | 1 核、initramfs shell | 无磁盘 | 启动到 `Alien:/#` |
| `no-nvme` | 1 核、initramfs shell | 无磁盘 | `NVMe: no device` |
| `aia1` | 1 核、initramfs shell | AIA | 中断控制台，IRQ 10 计数 |
| `smp2` | 2 核、initramfs shell | `QEMU_SMP=2` | `2/2 harts`，`nproc=2` |
| `aia` | 2 核、initramfs shell | AIA | 中断控制台，IRQ 10 计数 |
| `smp4` | 4 核、initramfs shell | `QEMU_SMP=4` | `4/4 harts`，`nproc=4` |
| `aia4` | 4 核、initramfs shell | AIA | `4/4 harts`，IRQ 10 计数 |
| `smp8` | 8 核、initramfs shell | `QEMU_SMP=8` | `8/8 harts`，`nproc=8` |
| `aia8` | 8 核、initramfs shell | AIA | `8/8 harts`，IRQ 10 计数 |
| `virtio-disk` | 1 核、FAT 磁盘 | virtio-blk | 挂载、读取、写入与读回 |
| `virtio-aia` | 1 核、FAT 磁盘 | AIA + virtio-blk | 中断控制台、挂载与写入 |
| `nvme-mount` | 1 核、FAT 磁盘 | NVMe | NVMe 注册、挂载与读取 |
| `nvme-aia` | 1 核、FAT 磁盘 | AIA + NVMe | 中断控制台、挂载与读取 |

磁盘场景使用 `K3_QEMU_DISK=1`，镜像中固定包含 `HELLO.TXT`。
`virtio-disk` 与 `virtio-aia` 还会写入 `WRITE.TXT` 并读回。

`high` 场景等待内核级 `kthread_init start...`。在 QEMU 高地址布局中，
K3 UART 地址由 RAM 覆盖，用户态输出不可见，因此该场景只断言高地址
内存、页表与帧分配器，不依赖用户态串口。

## 4. K3 上板场景

| 场景 | 构建 | 断言 |
|---|---|---|
| `boot` | 1 核 | 启动到 `Alien:/#`，标记命令返回 |
| `smp2` | 2 核 | `2/2 harts`，`nproc=2` |
| `smp4` | 4 核 | `4/4 harts`，`nproc=4` |
| `smp8` | 8 核 | `8/8 harts`，`nproc=8` |
| `aia` | 1 核 | 中断控制台，`42:` 计数 |
| `storage` | 1 核 | `/mnt` 挂载、`HELLO.TXT` 读取 |
| `uart` | 诊断 + UART 自测 | `K3_UART_READY`，回显 `uart-ok` |
| `pcie-recon` | 1 核 | 只读 U-Boot `pci`/`nvme` 取证，手动场景 |

`all` 默认排除 `pcie-recon`。该场景会复位 PCIe 链路并要求已安装 NVMe，
需要显式执行。上板运行器对已知的 TFTP 上传与以太网初始化抖动做一次
重试，默认 `--retries 1`：

```bash
./scripts/alien/test-k3-board.py pcie-recon --dry-run
./scripts/alien/test-k3-board.py pcie-recon
```

上板测试默认只读。当前没有自动写入 SD 卡或 NVMe 的场景；如需写入，
必须新增显式授权参数。

## 5. 构建与产物检查

`test-build.py` 覆盖：

| 场景 | 检查 |
|---|---|
| `qemu` | `kernel-qemu`、磁盘、initramfs、入口 `0x80200000` |
| `k3-qemu` | `kernel-k3-qemu`、磁盘、`build-info`、入口 `0x80200000` |
| `k3` | `kernel-k3`、FIT、raw/gzip、入口 `0x102200000`、`smp=1` |
| `k3-8core` | 八核 FIT、入口 `0x102200000`、`smp=8` |
| `k3-diag-uart` | 诊断 FIT、入口 `0x102200000`、`smp=1` |

FIT 通过 `mkimage -l` 校验，ELF 入口通过 `readelf -h` 校验。

## 6. 构建状态恢复

构建脚本会改写 `alien/subsystems/config/src/lib.rs` 的 `CPU_NUM`。
三个运行器都在每次构建后以及退出前恢复原始文件内容，避免把多核配置
留在工作区。

`build-k3-qemu.sh` 在 `alien/out-k3-qemu/build-info` 记录 `CPUS`、
`DISK` 与 `SHELL`。`test-qemu.py --no-build` 会校验产物变体；不匹配时
直接报告，不再让错误内核进入启动流程。

`build-k3.sh` 在 `alien/out-k3[-diag]/build-info` 记录 `CPUS`、`DIAG`
与 `UARTTEST`。`test-k3-board.py --no-build` 同样校验产物变体。

同一构建变体在连续场景之间复用；变体变化时才重新构建。上板场景按
1 核、2 核、4 核、8 核、诊断镜像顺序排列，减少重复构建。

## 7. 断言方式

QEMU 场景按以下顺序执行：

1. 等待启动标记，通常为 `Alien:/#`；`high` 为 `kthread_init start...`。
2. 检查场景要求的启动日志，例如 `NVMe: registered /dev/sda`。
3. 逐条发送命令，并等待对应输出。
4. 发送 `echo __TEST_"OK"__`，等待 `__TEST_OK__`。

标记命令故意带引号，使命令行回显为 `__TEST_"OK"__`，而实际输出为
`__TEST_OK__`，避免等待到命令回显而不是执行结果。

等待使用正则与轮询，不以固定 `sleep` 作为主要同步手段。单条命令默认
超时 10 秒，启动默认超时 90 秒。

## 8. 日志与结果

每次运行生成：

```text
logs/tests/<时间>-<场景>.log
logs/tests/build-<变体>.log
logs/tests/buildcheck-<场景>.log
logs/tests/<时间>-<场景>.runner.log
```

发生重试时，日志追加 `.attempt2` 等后缀，保留首次失败证据。

终端只打印摘要：

```text
[build] k3q-cpus1-disk-plain
PASS nvme-mount (0.9s)

15 scenario(s) passed
```

失败时打印最后 20 行并给出日志路径，退出码为 1。`logs/tests/` 已加入
`.gitignore`；需要长期保留的节选再整理到 `docs/agents/alien/logs/`。

## 9. 当前结果

2026-11-16 运行：

```text
./scripts/alien/test-qemu.py all --build
15 scenario(s) passed

./scripts/alien/test-build.py all
5 scenario(s) passed
```

QEMU 覆盖上游基线、高地址、1/2/4/8 核、AIA 1/2/4/8、virtio 读写、
virtio+AIA、NVMe 与 NVMe+AIA。

同一天执行 K3 默认上板场景：

```text
./scripts/alien/test-k3-board.py all --build
7 scenario(s) passed
```

通过的场景为 `boot`、`aia`、`storage`、`smp2`、`smp4`、`smp8`、`uart`。
其中 `aia`、`storage`、`smp2`、`smp4` 首次遇到上传或以太网抖动，均在
第二次尝试通过。`pcie-recon` 尚未执行，等待 NVMe 状态确认。

## 10. 历史测试覆盖对照

| 历史测试 | 当前覆盖 |
|---|---|
| 阶段 0 上游 QEMU 基线 | `qemu-baseline` |
| 阶段 2 `k3_qemu` 启动 | `baseline` |
| 阶段 3 高地址 QEMU | `high` |
| 阶段 4 QEMU bash | `baseline`、磁盘场景 |
| 阶段 5 QEMU 双核 | `smp2` |
| 阶段 6 QEMU 四核/八核 | `smp4`、`smp8` |
| 阶段 6 亲和性与负载 | 尚未覆盖 |
| 阶段 7 QEMU AIA 1/2/4/8 | `aia1`、`aia`、`aia4`、`aia8` |
| 阶段 8 QEMU virtio 读写 | `virtio-disk`、`virtio-aia` |
| 阶段 8 QEMU AIA + 磁盘 | `virtio-aia`、`nvme-aia` |
| 阶段 9A NVMe | `nvme-mount`、`nvme-aia`、`no-nvme` |
| 阶段 3–8 K3 上板 | `test-k3-board.py` 默认 7 场景已通过 |
| 构建、FIT、入口地址 | `test-build.py` |
| 构建干净克隆 | 尚未覆盖 |
| 亲和性、负载均衡、输出锁 | 尚未覆盖 |

## 11. 后续

- 确认 NVMe 状态后执行 `pcie-recon`，并把结果整理到阶段 9 记录。
- 如需覆盖 U-Boot 网络重试之外的失败，增加更细的板卡诊断。
- 评估是否需要自动化亲和性、负载均衡与输出锁测试。
- 如需要，把完整矩阵接入提交前检查。
