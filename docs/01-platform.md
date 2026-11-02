# 平台模型与认证

## 1. 你不是在 SSH 一台机器

课程/公共开发板平台给你的是三样东西：

```text
串口终端转发  +  远程电源管理  +  临时文件（TFTP）服务
```

- 每次 `ostool board connect` / `ostool board run` 都会**重新给板子上下电**。
- 上电后 U-Boot 有约 3 秒 autoboot 窗口。`board connect` 只做串口转发，**需要你手动按 `Ctrl+C` 打断**，否则板子会启动 SD 卡里预装的 Linux。
- 打断成功后停在 U-Boot 提示符 `=>`。
- `board run` 不同：它内部会周期性发送 `Ctrl+C` 自动打断 autoboot，不需要人工按键。
- 你的系统镜像不是从 SD 卡启动的，而是由 U-Boot 从网络（TFTP）拉到内存后启动。
- SD 卡上的数据跨会话保留，但**不要在本阶段写它**。

## 2. ostool 的三层配置

| 配置 | 默认路径 | 作用 |
|---|---|---|
| 全局服务 | `~/.ostool/config.toml` | `server`、`auth_mode` |
| 构建配置 | `.build.toml` 或 `--config` | 怎么构建，或直接用现成 ELF |
| 板卡配置 | `.board.toml` 或 `--board-config` | 板型、加载地址、U-Boot 准备命令、判活规则 |

本工作区的 Nix 包装器会把全局配置固定为：

```toml
[board]
server = "https://ostool.muxai.net:14321"
auth_mode = "required"
```

## 3. 认证

```bash
ostool login          # OAuth 设备授权：终端打印 URL + Code
ostool auth status    # 期望 credential: OAuth
ostool logout         # 清除本机凭据，不会结束管理员维护的租约
```

- 浏览器可以在另一台机器上打开，终端会等待授权完成。
- access token 到期后会自动刷新。
- 凭据优先存系统 keyring；不可用时退回 `~/.config/ostool/hosts.json`。
- 自动化场景可以用 `OSTOOL_BOARD_ACCESS_TOKEN`，该 token 不保存、不刷新。
- **任何 token / Code / hosts.json 内容都不要提交或截图外发。**

## 4. 租约不是连接

登录成功只代表有身份，不代表有板子。`ostool board ls` 能看到板型、可用数、租期，才说明管理员给你分配了有效租约。

连接时建议只写板型：

```bash
ostool board connect -b BananaPi
```

不要写 `--board-id`：平台显示的 BOARD ID 偶尔与实际分配不一致，交给服务端自动分配最稳。

## 5. 会话生命周期

- 客户端会发保活请求；正常退出时会尝试释放会话。
- 平台可能按策略强制回收会话（社区文档提到约 5 分钟，实际以平台为准）。
- 掉线、WebSocket reset 属于常见现象，重连即可；板子会重新上电。
- 长任务要设计成可重入：每段都从“重新上电 → U-Boot → 加载镜像”开始，不要把状态只放在内存里。

## 6. 文件传输

```text
本地文件 --HTTP PUT--> 会话目录 --TFTP--> 板子内存
```

- 单文件上限 **64 MiB**，超了会返回 HTTP 500，需要分块。
- TFTP 速度大约 4–8 MB/s（局域网），外网上传更慢。
- 上传接口返回的 JSON 里有 `tftp_url`，U-Boot 里用 `tftpboot` 拉取。
- 具体脚本见 `scripts/upload.sh`，细节见 `docs/02-uboot-cheatsheet.md`。

## 7. 退出

串口终端里：

```text
Ctrl+A  （松开）
x
```

不要直接 `kill` 管理会话的进程，否则客户端可能来不及请求释放会话。
