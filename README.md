# K3 远程开发板工作区

面向 BananaPi BPI-SM10（SpacemiT K3）的远程开发环境。
当前工作区包含 **登录 → 看租约 → 连串口 → 熟悉 U-Boot**、Alpine rootfs 实测，以及 Rust `riscv64gc-unknown-linux-musl` 交叉编译环境。

> 重要：平台实际有 Alpine 和 Yocto 两套镜像，`board connect` 自动分配且 `--board-id` 不可靠。详见 [`docs/04-k3-findings.md`](docs/04-k3-findings.md)。

## 快速开始

```bash
cd k3
nix develop

ostool --version          # 期望 0.31.1
ostool login              # 浏览器打开终端打印的 URL，输入 Code 授权
ostool auth status        # 期望 credential: OAuth
ostool board ls           # 期望能看到 BananaPi 且租期有效
ostool board connect -b BananaPi
```

看到 `Autoboot in 3 seconds` 时按 **`Ctrl+C`**，U-Boot 会打印 `<INTERRUPT>` 并出现：

```text
=>
```

> 实测 Enter/空格不能可靠打断；`Ctrl+C` 可以。错过 3 秒窗口，板子会启动 SD 卡里预装的 Linux，重连再来一次即可。

串口退出：**`Ctrl+A`，松开，再按 `x`**。

如果想留存完整会话日志：

```bash
./scripts/capture-session.sh
# 日志写入 logs/session-<时间戳>.log
```

想自动验证“能不能进 U-Boot”：

```bash
./scripts/smoke-test.py
# 自动按 Ctrl+C 打断 autoboot，跑 version 和 mmc list，然后退出
# 日志写入 logs/smoke-<时间戳>.log
```

## 目录

| 路径 | 用途 |
|---|---|
| `flake.nix` | devShell：ostool + Rust riscv64 musl 工具链 + curl/jq/python3/script/libsecret |
| `rust-hello/` | Rust 静态 riscv64 示例程序 |
| `.cargo/config.toml` | `rust-lld` + self-contained musl 交叉链接配置 |
| `docs/01-platform.md` | 平台模型、认证、会话生命周期、文件传输 |
| `docs/02-uboot-cheatsheet.md` | U-Boot 安全命令、网卡初始化、手动 booti |
| `docs/03-pitfalls.md` | 实测坑位合并表 |
| `docs/04-k3-findings.md` | 双板镜像、Alpine rootfs、串口/输入问题、Rust 交叉编译、当前恢复状态 |
| `docs/05-os-options.md` | OS 修复方案汇总：原地补 OpenRC、官方 rootfs、Debian、NixOS、initramfs、Yocto |
| `board/install.sh` | 板端增量安装脚本：OpenRC + getty + DHCP + sshd + chrony |
| `board/README.md` | 离线 bundle 的构建与板端执行说明 |
| `scripts/build-offline-bundle.sh` | 生成 `dist/k3-offline.tar.gz` 离线安装包 |
| `dist/` | 构建产物，已在 `.gitignore` 中忽略 |
| `scripts/capture-session.sh` | 用 `script(1)` 包住 `board connect`，留下 pty 日志 |
| `scripts/smoke-test.py` | 自动打断 autoboot，执行 `version` / `mmc list`，并干净退出 |
| `scripts/session-env.sh` | 从日志提取 `SID`，从 keyring / `hosts.json` 提取 token |
| `scripts/upload.sh` | 上传文件到当前会话，打印 `tftp_url` |
| `config/board.toml` | BananaPi `board run` 模板（阶段 2） |
| `config/prebuilt.toml` | `system.Custom` 预构建模板（阶段 2） |
| `logs/` | 会话日志，已在 `.gitignore` 中忽略 |

## 本阶段边界

- 只执行 U-Boot 只读命令：`version`、`printenv`、`bdinfo`、`mmc list`、`part list mmc 0`
- 不执行 `net list`、`saveenv`、`mmc write`
- 不构建内核、不运行 `ostool board run`
- 不修改 SD 卡内容
- 系统修复按 [`docs/05-os-options.md`](docs/05-os-options.md) 的方案 A 执行；写盘前先确认

## 关于配置

Nix 包里的 ostool 包装器每次启动都会把 `~/.ostool/config.toml` 替换成指向 Nix store 的符号链接，服务器固定为：

```toml
[board]
server = "https://ostool.muxai.net:14321"
auth_mode = "required"
```

所以不要指望用 `ostool board config` 持久化修改服务器；需要临时切换时用命令行 `--server`。

凭据优先存入系统 keyring，不可用时退回 `~/.config/ostool/hosts.json`。这些东西**不要提交到仓库**。

## 阶段 2（后续）

1. 在 `flake.nix` 里加 Rust 工具链（`rustup` + `riscv64gc-unknown-none-elf` + `llvm-tools`）或 `riscv64-unknown-elf-gcc`。
2. 用 `config/prebuilt.toml` 指向已构建内核，`config/board.toml` 提供启动参数。
3. 执行：

```bash
ostool board run \
  --config config/prebuilt.toml \
  --board-config config/board.toml
```

如果上传接口返回 401/token 失效，先执行一次 `ostool board ls` 让客户端刷新 token，再重新 `source scripts/session-env.sh`。也可以从 Web 管理台创建 PAT，然后：

```bash
export OSTOOL_BOARD_ACCESS_TOKEN='<pat>'
```
