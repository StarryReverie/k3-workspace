# U-Boot 速查（BananaPi BPI-SM10 / K3）

连接后先会看到 BootROM → SPL → U-Boot 滚屏。

## 1. 进入 U-Boot（关键）

`ostool board connect` 只转发串口，不会自动打断启动。看到下面这行时：

```text
[   6.715] Autoboot in 3 seconds
```

立刻按 **`Ctrl+C`**。U-Boot 会打印：

```text
=> [   6.727] <INTERRUPT>
=> 
```

出现 `=>` 就说明已经停在 U-Boot。

注意事项：

- **实测 Enter / 空格不能可靠打断，`Ctrl+C` 可以。**
- 连按多次 `Ctrl+C` 会打印多行 `<INTERRUPT>`，正常现象。
- 错过 3 秒窗口，板子会启动 SD 卡里预装的 Linux；断开重连（重新上电）再来一次。
- `ostool board run` 会在后台每 20 ms 发一次 `Ctrl+C` 自动打断，不需要你手动按；只有 `board connect` 需要人工按。

## 2. 安全只读命令

这些命令只读取信息，适合第一次熟悉板子：

```text
version                 # U-Boot 版本
printenv                # 全部环境变量
printenv bootcmd        # 默认启动命令
printenv ethaddr        # MAC（可能为空，再看 eth1addr）
bdinfo                  # 内存布局、DRAM 大小
mmc list                # 存储设备
part list mmc 0         # SD 分区表
help                    # 命令列表
```

本板实测输出示例：

```text
=> version
U-Boot 2022.10 (Apr 30 2026 - 11:07:27 +0800), Build: jenkins-BSP-build-bsp-deb-361
gcc (Bianbu 15.2.0-16ubuntu1bb2) 15.2.0
GNU ld (GNU Binutils for Bianbu) 2.46

=> mmc list
sdh@d4280000: 0
```

建议记录：U-Boot 版本、DRAM 大小、SD 设备号与分区起始 LBA。

## 3. 禁止 / 谨慎操作

| 命令 | 原因 |
|---|---|
| `net list` | 在 BPI-SM10 上会触发异常或挂起，已知问题 |
| 静态 `ipaddr` + `net list` | 官方文档确认在已验证固件上会挂起，不要复现 |
| `mmc write` | 参数必须带 `0x` 前缀，否则按十六进制误解析；本阶段不要写卡 |
| `saveenv` | 会把临时环境变量持久化，本阶段不需要 |
| `reset` | 会重启板子，虽然等价于重连，但没必要 |

## 4. 网卡初始化（阶段 2 由 ostool 自动执行）

官方 BananaPi 配置使用 DHCP 初始化，而不是静态 IP：

```text
test -n "$ethaddr" || setenv ethaddr $eth1addr
setexpr.l sm10_dline *0xd4282bf0
setexpr sm10_dline $sm10_dline "|" 0x10001
mw.l 0xd4282bf0 $sm10_dline 1
setenv netretry no && setenv autoload no && dhcp && setenv autoload yes
```

含义：

- `ethaddr` 为空时复用板卡已有的 `eth1addr`。
- 三条 `setexpr` / `mw.l` 是这块固件的网卡初始化变通：读 `0xd4282bf0`，按位或 `0x10001` 后写回。
- `netretry no` 避免网络失败后反复重启流程。
- `autoload no` 防止 DHCP 顺带下载默认启动文件。
- 成功后恢复 `autoload yes`。

如果只是想手动确认网络：

```text
setenv serverip 192.168.1.2
setenv ipaddr 192.168.1.157
setenv netmask 255.255.255.0
ping 192.168.1.2
```

但注意：设置 `ipaddr` 不会完成网卡硬件初始化；在已验证固件上，静态 IP 后执行 `net list` 会挂起。正常部署交给 ostool 的 `uboot_cmd`。

## 5. 手动加载与启动（阶段 2）

先通过 HTTP API 上传内核和 DTB，再从 U-Boot 拉取：

```text
tftpboot 0x102200000 ostool/sessions/<SESSION_ID>/boot/starryos.bin
tftpboot 0x110000000 ostool/sessions/<SESSION_ID>/boot/board.dtb
booti 0x102200000 - 0x110000000
```

BananaPi 常用地址：

| 用途 | 地址 |
|---|---|
| 内核加载 | `0x102200000` |
| DTB | `0x110000000` |
| FIT 加载 | `0x140000000` |
| `bootm` | `0x140000000` |

这些地址必须与服务端板卡配置一致，不要从其他板型复制。

## 6. 判断启动进度

| 日志 | 含义 |
|---|---|
| `Bytes transferred = ...` | TFTP 传输完成 |
| `Starting kernel ...` | U-Boot 已跳转，不代表内核已起来 |
| `root@starry:` | StarryOS shell 就绪 |
| `EQOS_DMA_MODE_SWR stuck` | 网卡 DMA 复位失败，通常重连（重新上电）可恢复 |

## 7. 退出

```text
Ctrl+A  （松开）
x
```

正常退出会看到：

```text
Exit by: Ctrl+A+x
✓ 已退出串口终端模式
```
