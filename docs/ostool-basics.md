# 用 ostool 操作 K3 开发板

适用硬件：BananaPi BPI-SM10（SpacemiT K3）
适用系统：已完成修复的 Alpine 3.23.4，已安装 OpenRC、串口 getty、dhcpcd、sshd 与 chrony
适用工具：ostool 0.31.1

本文介绍从连接开发板到在系统中执行命令、传输文件的完整流程，不涉及自行编译内核与镜像加载。文中开发板内的命令均以修复后的 Alpine 镜像为准；两套镜像的特征见 5.3 节。

## 1. 平台提供的能力

ostool 提供的是到开发板的串口转发，不是 SSH 通道。平台提供三项彼此独立的服务：

```text
串口终端转发  +  远程电源控制  +  临时文件服务
```

开发板内可以运行 sshd，但宿主机没有通往实验网络的路由，无法直接 SSH 登录。串口是唯一的交互入口。

由此产生三项硬性限制，后续所有操作都受其约束。

| 限制 | 具体表现 |
|---|---|
| 每次连接都会重新上电 | 开发板从 BootROM 重新启动，内存中的内容不会保留 |
| 会话存在时限 | 平台会回收会话，社区文档提到约 5 分钟，实际以平台为准 |
| 开发板没有外网 | 实验网络没有默认路由与 DNS，只有 `192.168.1.2:2999` 的会话共享可以访问 |

## 2. 安装与登录 ostool

### 2.1 进入开发环境

本工作区通过 Nix 提供 ostool 与交叉编译工具链：

```bash
cd k3
nix develop
ostool --version
```

期望输出 `0.31.1`。版本低于 0.31 时，板卡配置中的 `shell_check_steps` 不会被识别。

### 2.2 授权

```bash
ostool login
ostool auth status
```

`ostool login` 采用设备授权：终端打印一个网址与一次性代码，在任意一台可以联网的电脑上打开该网址并输入代码即可完成授权。`auth status` 的输出中，`credential` 应为 `OAuth`。

凭据优先保存在系统密钥环；密钥环不可用时退回 `~/.config/ostool/hosts.json`。凭据、登录代码与访问令牌都不要写入仓库，也不要对外发送。

### 2.3 确认已分配开发板

授权成功仅表示身份有效，不代表已经分配到开发板。

```bash
ostool board ls
```

输出中应包含 `BananaPi` 以及有效租期。若列表为空或租期已过，需要联系平台管理员重新分配。

## 3. 连接开发板

```bash
ostool board connect -b BananaPi
```

只指定板型，不要附加 `--board-id`。平台显示的编号偶尔与实际分配的开发板不一致，指定编号可能被拒绝，也可能连接到另一块开发板。

连接后，串口依次输出 BootROM、SPL 与 U-Boot 的启动信息。U-Boot 在自动启动之前留有约 3 秒的等待窗口：

```text
Autoboot in 3 seconds
```

此时按下 `Ctrl+C`，U-Boot 输出 `<INTERRUPT>` 并停留在提示符：

```text
=>
```

实测 Enter 与空格无法可靠中断，必须使用 `Ctrl+C`。连续按多次会输出多行 `<INTERRUPT>`，属于正常现象。错过该窗口后，开发板会继续启动 SD 卡中的系统；断开并重新连接即可重新上电。

若后续需要传输文件，第 6 章需要从会话日志中提取会话编号，因此连接时应改用：

```bash
./scripts/capture-session.sh
```

该脚本用 `script(1)` 包装 `ostool board connect`，在保留完整日志的同时提供伪终端。ostool 仅在标准输入与标准输出均为终端时转发串口输入；通过管道直接调用不会报错，但输入不会被转发。

会话的时限已在第 1 章说明。长时间操作应设计为可以重新开始：每一段都从重新上电开始，再进入 U-Boot 并启动系统，不要假设内存中的状态会保留。

## 4. 在 U-Boot 中查看开发板信息

停留在 `=>` 之后，建议先执行只读命令确认硬件与固件版本。这些命令不会修改任何状态。

```text
version
bdinfo
mmc list
part list mmc 0
printenv bootcmd
printenv commonargs
```

本开发板的实测输出：

```text
=> version
U-Boot 2022.10 (Apr 30 2026 - 11:07:27 +0800), Build: jenkins-BSP-build-bsp-deb-361
gcc (Bianbu 15.2.0-16ubuntu1bb2) 15.2.0
GNU ld (GNU Binutils for Bianbu) 2.46

=> mmc list
sdh@d4280000: 0
```

第 5 章会用到 `bootcmd` 与 `commonargs` 的值，建议记录。`mmc list` 只列出存储设备，分区表需要使用 `part list mmc 0`。

以下命令在本开发板上不可执行或不应执行：

| 命令 | 原因 |
|---|---|
| `net list` | 在这块固件上会触发异常或挂起 |
| 设置静态 `ipaddr` 后执行 `net list` | 静态地址不会完成网卡初始化，同样会挂起 |
| `saveenv` | 会将临时环境变量写入持久存储；每次会话都应保持服务端提供的初始环境 |
| `mmc write` | 本文不涉及写入 SD 卡；该命令的参数还必须带 `0x` 前缀，否则按十六进制解析出错 |

## 5. 启动系统并登录

### 5.1 默认启动

```text
run bootcmd
```

`bootcmd` 的值为 `run autoboot; echo "run autoboot"`，即执行正常的启动流程。

启动参数由 `commonargs` 决定。`commonargs` 的值为 `setenv bootargs splash`，它在启动开始时重新设置 `bootargs`，因此在 `=>` 下直接执行 `setenv bootargs ...` 不会生效。需要追加参数时，应修改 `commonargs`：

```text
setenv commonargs 'setenv bootargs splash init=/bin/sh rw'
run bootcmd
```

上例仅用于说明参数注入的位置，正常使用不需要修改。

### 5.2 登录

启动完成后，串口显示：

```text
k3 login:
```

以 `root` 登录。口令由管理员另行提供，不写入仓库。

### 5.3 两套系统镜像

平台至少有两块 BananaPi，镜像不同。`board connect -b BananaPi` 由服务端分配，无法指定具体一块：

| 板型 | 系统 | 特征 |
|---|---|---|
| BananaPi-1 | Alpine 3.23.4 + OpenRC | 串口登录由 `ttyS0` 上的 getty 提供 |
| BananaPi-2 | Yocto/Poky 5.3.99 + systemd | 串口登录由 `serial-getty@ttyS0.service` 提供 |

两者都能正常登录，但可用的命令与服务管理方式不同。可在系统中查看 `/etc/os-release` 确认当前镜像。第 6 章的命令适用于表中的 Alpine 镜像。

### 5.4 串口输入

`ttyS0` 没有硬件流控，输入并不可靠，表现为两类问题：

- BusyBox ash 在显示提示符之前发送 `ESC[6n` 查询光标位置。串口另一端若不应答，行首会丢失一个字符，偶尔多出一个 `[`。
- 一次性粘贴较长内容时，字符以突发方式写入，超出串口接收能力，中间的字符会被丢弃。

实际处理方式：

- 命令分行输入，每行结束后确认回显正确再执行。
- 避免粘贴长 URL；可将地址拆成变量，或改用第 6 章的上传脚本。
- 出现字符丢失时，重新输入整行，不要依赖光标移动修正。

## 6. 网络配置与文件传输

### 6.1 确认网络

Alpine 镜像正常启动后，`dhcpcd` 已为 `end1` 取得地址：

```sh
ip addr show end1
```

通常可以看到 `192.168.1.157/24`。若没有地址：

```sh
ip link set end1 up
rc-service dhcpcd restart
ip addr show end1
```

实验网络仅在 `192.168.1.0/24` 网段内部可达，其中 `192.168.1.2:2999` 提供当前会话的文件共享。该网络没有默认路由，也没有 DNS，因此 `apk add`、访问公网地址等操作都会失败。安装软件需要预先准备离线包。

### 6.2 从宿主机上传

上传通过 HTTP 接口完成，文件写入当前会话的目录：

```text
宿主机文件 --HTTP PUT--> 会话目录 --HTTP 共享--> 开发板
```

先导出当前会话的凭证。脚本从会话日志中提取会话编号，从系统密钥环或 `hosts.json` 中提取访问令牌：

```bash
source scripts/session-env.sh
```

然后上传：

```bash
./scripts/upload.sh path/to/file boot/file
```

第二个参数是会话内的相对路径，省略时默认为 `boot/<文件名>`。接口返回的 JSON 中包含 `tftp_url`；`http_url` 可能为空，开发板内下载使用 6.3 节的共享地址。

限制与注意事项：

- 单个文件不得超过 64 MiB，超过时接口返回 HTTP 500。
- 会话编号来自日志。若第 3 章使用的是 `ostool board connect` 而非 `capture-session.sh`，则没有日志可供提取，需要重新连接。
- 若日志不是当前会话产生的，下载会返回 404，需要重新执行 `source scripts/session-env.sh`。
- 上传返回 401 表示令牌失效。先执行 `ostool board ls` 刷新，再重新导出凭证。

### 6.3 在开发板内下载

会话目录通过 `192.168.1.2:2999` 以只读方式共享，地址格式固定：

```text
http://192.168.1.2:2999/share/sessions/<会话编号>/boot/<文件名>
```

```sh
wget -O /root/program 'http://192.168.1.2:2999/share/sessions/<会话编号>/boot/program'
```

下载完成后先校验，再赋予执行权限：

```sh
sha256sum /root/program
chmod +x /root/program
/root/program
```

`/run` 挂载时带 `noexec`，可执行文件不要放在该目录下；`/root`、`/tmp`、`/usr/local/bin` 均可。

## 7. 退出会话

在串口终端中先按 `Ctrl+A`，松开后再按 `x`：

```text
Ctrl+A  （松开）
x
```

正常退出时输出：

```text
Exit by: Ctrl+A+x
✓ 已退出串口终端模式
```

不要直接终止运行 `ostool` 的进程。客户端需要在退出时请求释放会话，强制终止可能导致会话被占用，直到平台回收。

退出后会话结束，SD 卡中的数据保留。

## 8. 遇到问题时的排查顺序

| 现象 | 处理 |
|---|---|
| 连接后没有出现 `=>`，直接进入系统 | 错过 3 秒窗口。重新连接后立即按 `Ctrl+C`，参见第 3 章 |
| 串口会话突然中断 | 平台回收了会话。重新连接即可，SD 卡中的数据保留 |
| 指定 `--board-id` 被拒绝，或分配到的开发板与预期不符 | 去掉 `--board-id`，只使用 `-b BananaPi`，参见第 3 章 |
| 下载文件返回 404 | 会话编号与当前会话不一致。重新执行 `source scripts/session-env.sh`，参见第 6 章 |
| 上传返回 500 | 文件超过 64 MiB，需要拆分，参见第 6 章 |
