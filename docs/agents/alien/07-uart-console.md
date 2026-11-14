# K3 控制台输入与 `0x01` 洪泛修复

日期：2026-11-14
范围：修复完整镜像启动 bash 后的 `0x01` 洪泛，接入 K3 轮询 UART，验证交互输入
不涉及：中断驱动 UART、IMSIC/APLIC、SMP、磁盘与网络

## 1. 问题

阶段 2 与阶段 3 使用 SBI 控制台 `/dev/tty`：

```text
SbiConsoleDevice:
  write_at -> platform::console_write
  read_at  -> platform::console_getchar
```

完整镜像包含 bash 时，启动后出现连续 `0x01`：

```text
Init process is running
Alien:/# \x01\x01\x01\x01...
```

诊断镜像不启动 bash，因此没有触发。

## 2. 定位过程

### 2.1 补齐 termios ioctl

首先确认 `SbiConsoleDevice` 缺少终端 ioctl。新增：

```text
TCGETS / TCGETA
TCSETS / TCSETSW / TCSETSF
TIOCGPGRP / TIOCSPGRP
TIOCGWINSZ / TIOCSWINSZ
```

使用 `Termios::default()` 与 `WinSize::default()`，默认 80x24、`ICANON | ECHO | ISIG`。

修复后提示符可以正常出现：

```text
Alien:/#
```

但随后仍然出现 `0x01`，说明 ioctl 只解决了 bash 的终端识别，不是洪泛根因。

### 2.2 排除内核输出路径

在 QEMU 中使用 GDB 与串口 trace 检查：

- `serial_write [0x00] <- 0x01` 连续出现，确认 guest 在写 UART THR。
- 断点 `SbiConsoleDevice::write_at`：提示符阶段命中，洪泛阶段不命中。
- 断点 `Stdout::write_str`：启动阶段命中，洪泛阶段不命中。
- 断点 `core::fmt::Write::write_char`：洪泛阶段不命中。
- 断点 SBI putchar 入口：洪泛阶段不命中。
- CPU 采样：大量时间停在 OpenSBI 的 UART 轮询与输出循环。
- 内核 `read_at` 正在回退到 SBI legacy getchar。

### 2.3 确认根因

临时把 `SbiConsoleDevice::read_at` 改成只自旋、不调用 `console_getchar`，重新构建后：

```text
Init process is running
Alien:/#
0x01 数量：0
```

由此确认：

**洪泛来自 SBI 控制台输入轮询路径，不是 bash 写输出，也不是内核 `console_putchar`。**

SBI legacy getchar 在轮询过程中触发了固件侧的控制台输出，最终以 `0x01` 出现在串口。

## 3. 修复方案

不再让用户态终端依赖 SBI 输入，改用真实 UART 轮询。

### 3.1 PollingUart

在 `subsystems/devices/src/uart.rs` 增加 `PollingUart`：

- 实现 `UartDevice` 与 `DeviceBase`。
- `get()` 直接轮询底层 `LowUartDriver::_read()`。
- `put()` / `put_bytes()` 直接写底层 UART。
- `have_data_to_get()` 带一个 pending 字节缓存。
- 不启用 UART 接收中断，避免在没有 IMSIC 的情况下产生未处理中断。

### 3.2 平台与内核接入

`subsystems/devices/src/lib.rs` 增加：

```text
init_k3_uart(0xd4017000)        K3，Uart8250
init_k3_qemu_uart(0x10000000)   QEMU，Uart16550
```

`kernel/src/lib.rs` 在 VFS 初始化前注册 `UART_DEVICE`，使 `init_devfs` 使用 `UARTDevice` 创建 `/dev/tty`。

`k3_riscv/config.rs` 增加 MMIO 映射：

```text
K3:        0xd4017000..0xd4018000
K3 QEMU:   0x10000000..0x10001000
```

长度必须是 4 KiB 对齐，否则内核页表映射返回 `NotAligned`。

### 3.3 UARTDevice 加固

`UARTDevice::ioctl` 原来使用：

```rust
TeletypeCommand::try_from(cmd).unwrap()
...
unimplemented!("ioctl cmd: {:?}", cmd)
```

改为未知命令返回 `VfsError::NoSys`，避免 bash 发送未知 ioctl 时内核 panic。

### 3.4 测试开关

- `K3_UARTTEST=1`：诊断镜像加入 `/tests/k3-uarttest`，init 打印 `K3_UART_READY` 后逐字节读取并回显。
- `K3_QEMU_SHELL=1`：`k3_qemu` 镜像包含 bash，用于 QEMU 交互回归。
- `boot-k3-board.py --post-command`：在上板标记出现后发送命令。

## 4. 验证结果

### 4.1 QEMU 完整 bash

```text
Alien:/# echo K3_OK
K3_OK
Alien:/# /bin/busybox ls /
bin    dev    etc    proc   root   sys    tests  tmp    var
Alien:/#
```

日志中 `0x01` 数量为 0。详见 `logs/2026-11-14-k3-qemu-bash.txt`。

### 4.2 K3 UART 输入测试

```text
Init process is running
K3_UART_READY
uuaarrtt--ookk
```

`uuaarrtt--ookk` 是双重回显：

- 内核 `UARTDevice` 的 `ECHO`。
- init 自己的逐字节回显。

说明 K3 的轮询 UART 可以接收输入。日志中 `0x01` 数量为 0。详见 `logs/2026-11-14-k3-uart.txt`。

### 4.3 K3 完整 bash

```text
Init process is running
Alien:/# echo K3_OK
K3_OK
Alien:/# /bin/busybox ls /
bin    dev    etc    proc   root   sys    tests  tmp    var
Alien:/#
```

日志中 `0x01` 数量为 0。详见 `logs/2026-11-14-k3-bash.txt`。

### 4.4 回归

```text
QEMU 基线           Alien:/#
k3_qemu 诊断镜像    K3_QEMU_INIT_OK
完整 K3 镜像        构建成功
```

## 5. 上板时间线

一次 2.6 MiB 诊断镜像上板：

```text
连接开始
→ U-Boot 提示符       9 秒
→ 上传完成            34 秒
→ 网络初始化完成      11 秒
→ tftpboot + bootm    2 秒
→ K3_UART_READY       0.4 秒
→ 命令执行与退出      3 秒
合计                  约 62 秒
```

一次 5.2 MiB 完整镜像上板：

```text
连接开始
→ U-Boot 提示符       15 秒
→ 上传完成            98 秒
→ 网络初始化与启动    13 秒
→ Alien:/#            0.5 秒
→ 命令执行与退出      4 秒
合计                  约 135 秒
```

上传速率约 50–90 KB/s，波动明显；历史记录中 2.68 MiB 也需要 38–61 秒。瓶颈在宿主机到 ostool 服务端，不在板端 TFTP（约 80 MiB/s）。

### 5.1 超时设置

`boot-k3-board.py` 当前默认：

```text
上传超时      120 秒
整体上限      240 秒
bootm 后等待  20 秒
命令后等待    2 秒
```

脚本在命令执行完成后立即退出，整体上限只是安全边界。

### 5.2 已修复的脚本问题

早期版本把“发送后续命令”的代码放在 `select` 之后。串口空闲时 `select` 超时会 `continue`，导致命令永远不发送，脚本只能等到整体超时。现已移到 `select` 之前，并增加墙钟时间戳。

## 6. 已知问题

- 内核日志仍通过 SBI DBCN 输出，用户态 `/dev/tty` 使用 UART MMIO，同一 UART 有两个写入者。
- `PollingUart::get()` 在无输入时忙等，未接入 IMSIC 中断。
- 没有流控与终端窗口大小变化的实际处理。
- 完整镜像上传仍然较慢；后续可考虑 FIT gzip 压缩。
- `/bin/ls` 不存在，测试命令使用 `/bin/busybox ls`。
- 未验证 Ctrl-C、作业控制与多任务交互。

## 7. 复现命令

```bash
# QEMU 完整 bash 交互
K3_QEMU_SHELL=1 ./scripts/alien/build-k3-qemu.sh
# 用 pty 运行 out-k3-qemu/kernel-k3-qemu

# K3 UART 输入自测
K3_DIAG=1 K3_UARTTEST=1 ./scripts/alien/build-k3.sh
./scripts/alien/boot-k3-board.py \
  --image alien/out-k3-diag/alien-k3-diag.itb \
  --marker K3_UART_READY \
  --post-command "uart-ok"

# K3 完整 bash
./scripts/alien/build-k3.sh
./scripts/alien/boot-k3-board.py \
  --image alien/out-k3/alien-k3.itb \
  --marker "Alien:/#" \
  --post-command "echo K3_OK" \
  --post-command "/bin/busybox ls /"
```
