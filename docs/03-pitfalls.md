# 已知坑位合并表

来源：

- hera-rt-lab《远程使用 K3 开发板帮助文档》
- M7OS《K3 运行 RME》
- TGOSKits 官方《ostool 使用》

按出现频率排序。

## 先看这三条

1. **`board connect` 不会自动停在 U-Boot。** 看到 `Autoboot in 3 seconds` 时按 `Ctrl+C`（实测 Enter/空格无效）；`board run` 会自动每 20 ms 发一次 `Ctrl+C`，不需要人工按。
2. **不要手动执行 `net list`。** 网卡初始化和 TFTP 上传交给 ostool。
3. **串口退出是 `Ctrl+A` 松开再按 `x`。** 不要直接 kill 进程。

| # | 现象 | 原因 / 处理 |
|---|---|---|
| 1 | 串口会话突然断开（websocket reset） | 平台会强制回收会话（社区文档提到约 5 分钟）。重连会重新上电；SD 数据保留 |
| 2 | 上传文件返回 HTTP 500 | 单文件超过 64 MiB。切块上传 |
| 3 | 指定 `--board-id` 被拒，提示分配了另一块板 | 平台显示的 BOARD ID 偶尔与实际租约不一致。去掉 `--board-id`，只用 `-b BananaPi` |
| 4 | `board run` 报 `shell check sequence ended before ... completed` | 会话在判活瞬间被回收。如果日志里已经出现成功标记，实际可能已经成功 |
| 5 | `mmc write` 报 `block number exceeds max` | U-Boot 参数必须带 `0x` 前缀，例如 `mmc write 0x180000000 0x2000 0x20000` |
| 6 | TFTP 报 `EQOS_DMA_MODE_SWR stuck -110` | 网卡初始化抖动，约半数上电出现。断开重连（重新上电）多试几次 |
| 7 | U-Boot 里 `net list` 后板子复位或挂起 | 该命令在这块板上有已知问题，不要手动执行；初始化和上传交给 ostool |
| 8 | 静态 `ipaddr` 后 `net list` 挂起 | 静态 IP 不会完成网卡硬件初始化。用官方 `uboot_cmd` 的 DHCP 流程 |
| 9 | 0.31+ 报 “removed U-Boot config key” | 顶层 `shell_prefix` / `shell_init_cmd` / `success_regex` 已删除，必须迁移到 `shell_check_steps` |
| 10 | 非 Rust 内核跑 `board run` 报找不到 Cargo 工作区 | ostool 需要 `Cargo.toml` 和 `src/lib.rs` 占位（M7 RME 文档的实测结论） |
| 11 | 从 `script` 日志提取 SID 后 URL 报格式错误 | pty 日志行尾有 `\r`，用 `tr -d '\r'` |
| 12 | 上传 API 时通时断（HTTP 000） | 脚本缓存了旧 token。每次重新从 `hosts.json` / `OSTOOL_BOARD_ACCESS_TOKEN` 读取 |
| 13 | 内核访问 SD 卡报 CRC mismatch 后持续超时 | 原内核 SDHCI 驱动缺延迟调谐；hera 项目的补丁包含移植。先重上电 |
| 14 | StarryOS 里向量指令程序 SIGILL（exit 132） | 内核未启用用户态向量扩展；同上补丁包含 `sstatus.VS` 使能和向量上下文保存 |
| 15 | `ostool board config` 改完服务器，下次启动又变回去 | 本仓库的 Nix 包装器会覆盖 `~/.ostool/config.toml`。要换服务器请改 Nix 包或用 `--server` |
| 16 | 正常启动连续 3 次 `can't run '/sbin/openrc'`，串口没有 shell | Alpine rootfs 没装 OpenRC，但 inittab 仍是 OpenRC 配置；用 `init=/bin/sh`，或自行改 inittab（先备份） |
| 17 | 执行过 `stty -echo` 后，后续 shell 输入没有回显 | tty 设置会保留；执行 `stty echo icanon` |
| 18 | 同一个 `-b BananaPi` 有时是 Alpine，有时是 Yocto，行为完全不同 | 平台至少有两块板、两套镜像；`--board-id` 不可靠；从 `board_id` 和提示符判断 |
| 19 | 板子 Linux 里 `tftp: not found` | Alpine BusyBox 没有 tftp/ftpget；U-Boot TFTP 仍可用，Linux 内可用 `wget` HTTP 共享或 initramfs |
| 20 | `setenv bootargs ... init=/bin/sh` 不生效 | `commonargs` 会重置 `bootargs`；应改 `commonargs`，见 `docs/04-k3-findings.md` |

## 本工作区额外约定

- 不在 U-Boot 里执行 `saveenv`，保持每次会话从服务端提供的环境开始。
- 不手动执行 `net list`，网络初始化只走 `uboot_cmd`。
- 所有会话日志放在 `logs/`，不进版本库。
- 任何 token、Code、`hosts.json` 内容都不写进仓库。
