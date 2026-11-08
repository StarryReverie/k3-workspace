# 修复 K3 开发板上的 Alpine 系统

适用硬件：BananaPi BPI-SM10（SpacemiT K3），BananaPi-1
适用系统：Alpine 3.23.4，根文件系统缺少启动组件
修复结果：正常启动至 `k3 login:`，OpenRC、串口 getty、dhcpcd、sshd 与 chrony 可用

本文记录从故障判定到安装、验证与回滚的完整过程。本文只补充安装启动组件，不修改分区表、内核、设备树与 initramfs。

## 1. 故障判定

### 1.1 启动日志中的特征

正常上电后，U-Boot 移交启动，内核与 initramfs 完成根文件系统挂载，随后串口连续输出：

```text
can't run '/sbin/openrc': No such file or directory
```

该行出现三次，之后没有登录提示，也没有可用的 shell。这是本文处理的情况。

若能正常看到 `k3 login:`，说明系统已经具备启动组件，本文不适用。平台还可能分配到 Yocto 镜像，该镜像可以正常登录，同样不属于本文范围。

### 1.2 原因

根文件系统是按容器镜像的方式制作的：基础目录与软件包管理工具齐全，部分开发工具也已安装，但缺少 Alpine 启动所需的 OpenRC（服务管理程序）与服务脚本。

具体表现为：

- `/sbin/init` 指向 BusyBox，而 `/etc/inittab` 调用 `/sbin/openrc`
- `/etc/init.d` 不存在
- `/etc/inittab` 中 `ttyS0` 的 getty（登录程序）一行处于注释状态
- root 账户处于锁定状态

因此系统能够挂载根文件系统，却无法进入运行级别（runlevel），也无法在串口上提供登录界面。

文件系统中的确认在第 4 章进入 shell 之后进行。

## 2. 修复范围与可行性

### 2.1 在现有根文件系统上安装

可行的做法有两种：重新制作根文件系统，或在现有根文件系统上补充安装启动组件。本文选择后者。

理由如下：

- 现有根文件系统中的 `gcc`、`git`、`vim` 等工具仍然可用，重新制作会丢失这些内容
- 不需要写分区表，也不需要重新制作文件系统
- 启动链、内核与硬件驱动完全不动
- 即使安装失败，仍可通过 `init=/bin/sh` 进入并恢复

### 2.2 改动范围

需要安装：

- OpenRC 与 BusyBox 的 OpenRC 支持包
- `ttyS0` 上的 getty
- `dhcpcd`（DHCP 客户端）、`sshd`（SSH 服务）、`chrony`（时间同步）
- 标准的运行级别与服务项

不改动：

- 分区表与 `/dev/sda3` 的布局
- 内核、设备树、initramfs
- U-Boot 环境变量
- Yocto 镜像所在的另一块开发板

### 2.3 为什么必须离线安装

开发板没有默认路由，也没有可用的 DNS，只能访问 `192.168.1.0/24` 网段内的 `192.168.1.2:2999`。因此 `apk`（Alpine 的软件包管理工具）无法从官方镜像站下载软件包，安装包必须先在宿主机准备好，再通过会话共享传入开发板。

## 3. 在宿主机构建离线安装包

### 3.1 来源

Alpine 官方的 U-Boot 发布包自带一套离线仓库：

```text
https://dl-cdn.alpinelinux.org/alpine/v3.23/releases/riscv64/alpine-uboot-3.23.7-riscv64.tar.gz
```

其中 `apks/riscv64/` 约 17 MB，包含 100 余个软件包，版本为 3.23.7，与开发板上的 3.23.4 属于同一分支。

### 3.2 构建

构建脚本需要 `curl` 与 `openssl`。root 口令通过环境变量传入，不写入仓库：

```bash
cd k3
read -rs K3_ROOT_PASSWORD
export K3_ROOT_PASSWORD
./scripts/build-offline-bundle.sh
```

`read -rs` 不回显输入，也不会把口令写入 shell 历史。

如需同时安装 SSH 公钥：

```bash
K3_AUTHORIZED_KEYS=~/.ssh/id_ed25519.pub ./scripts/build-offline-bundle.sh
```

### 3.3 产物

```text
dist/k3-offline.tar.gz
```

包内包含：

```text
offline/install.sh
offline/root.hash
offline/README.txt
offline/apks/riscv64/APKINDEX.tar.gz
offline/apks/riscv64/*.apk
```

`root.hash` 是 `$6$` 格式的口令哈希，明文口令不在包内。

构建脚本会输出文件大小与 sha256。安装包包含口令哈希，口令不同则校验值不同，因此校验值以本次构建的输出为准，不要沿用旧值。包大小必须低于 64 MiB，脚本会检查。

## 4. 以 init=/bin/sh 启动

### 4.1 连接并保留日志

```bash
./scripts/capture-session.sh
```

使用该脚本而不是 `ostool board connect`，是因为第 5 章上传安装包时需要从会话日志中提取会话编号。

### 4.2 注入启动参数

看到 `Autoboot in 3 seconds` 时按 `Ctrl+C`，在 `=>` 下执行：

```text
setenv commonargs 'setenv bootargs splash init=/bin/sh rw'
run bootcmd
```

启动流程会先执行 `commonargs`，而 `commonargs` 会重新设置 `bootargs`。直接执行 `setenv bootargs ...` 不会生效，参数必须写入 `commonargs`。

### 4.3 确认系统身份

`init=/bin/sh` 不经过 OpenRC，也不提供登录界面，启动完成后直接得到一个 root shell。

先执行：

```sh
mount -o remount,rw /
stty echo icanon
dmesg -n 1
```

`mount -o remount,rw /` 是因为厂商提供的 initramfs 结束时会以只读方式挂载根文件系统。`stty echo icanon` 用于清除上一次会话可能残留的终端设置。`dmesg -n 1` 减少内核日志插入串口输入。

随后确认系统：

```sh
cat /etc/os-release
ls /etc/init.d
```

`ID=alpine` 且 `/etc/init.d` 不存在，说明与第 1 章描述的状态一致。若 `ID` 不是 `alpine`，说明分配到 Yocto 镜像，应停止操作并重新连接。

## 5. 上传并安装

### 5.1 从宿主机上传

在宿主机上另开一个终端，并进入同一开发环境：

```bash
source scripts/session-env.sh
./scripts/upload.sh dist/k3-offline.tar.gz boot/k.tar.gz
```

脚本会输出会话编号、上传结果与 `tftp_url`。记下会话编号，下一步的下载地址需要用到。

### 5.2 在开发板内下载

```sh
ip link set end1 up
udhcpc -i end1 -n -q -t 3 -T 2
wget -O /root/k.tar.gz 'http://192.168.1.2:2999/share/sessions/<会话编号>/boot/k.tar.gz'
sha256sum /root/k.tar.gz
```

下载地址中的 `<会话编号>` 需要替换为实际值。`sha256sum` 的输出应与第 3 章构建时打印的校验值一致。

`udhcpc` 只负责取得地址，不会写入默认路由与 DNS；本文只需要访问同一网段内的 `192.168.1.2`，因此不需要这两项。

### 5.3 解压并执行

```sh
tar xzf /root/k.tar.gz -C /root
sh /root/offline/install.sh
```

安装脚本依次完成以下操作：

1. 将根文件系统重新挂载为可读写
2. 备份 `/etc` 到 `/root/etc-backup.tar.gz`
3. 把本地软件包仓库写入 `/etc/apk/repositories`，原有地址保留为注释
4. 安装软件包
5. 重写 `/etc/inittab`，启用 `ttyS0` 上的 getty
6. 将 `ttyS0` 加入 `/etc/securetty`
7. 设置主机名、`/etc/hosts` 与 root 口令哈希
8. 生成 SSH 主机密钥，写入可选公钥，设置 `PermitRootLogin prohibit-password`
9. 将服务加入运行级别
10. 同步磁盘并输出 `INSTALL_DONE`

安装的软件包：

```text
alpine-base openrc ifupdown-ng busybox-openrc busybox-mdev-openrc
openssh openssh-server-common-openrc openssh-keygen
dhcpcd dhcpcd-openrc chrony chrony-openrc
e2fsprogs e2fsprogs-extra doas ca-certificates-bundle tzdata wget
```

### 5.4 重启

```sh
sync
tail -n 5 /root/k3-install.log
reboot -f
```

日志末尾应出现 `== INSTALL_DONE ==`。没有出现时不要重启，先查看 `/root/k3-install.log` 中的错误行。

## 6. 验证

重新连接，不要在 `Autoboot in 3 seconds` 时按 `Ctrl+C`，等待系统自行启动。启动完成后应看到：

```text
k3 login:
```

以 `root` 登录，口令为第 3 章构建时设置的值。随后逐项检查：

```sh
rc-status
rc-service dhcpcd status
rc-service sshd status
rc-service chronyd status
ip addr show end1
ping -c 3 192.168.1.2
df -h /
```

期望结果：

| 检查项 | 期望 |
|---|---|
| `rc-status` | `default` 运行级别中包含 `dhcpcd`、`sshd`、`chronyd`、`crond` |
| `rc-service dhcpcd status` | `started` |
| `rc-service sshd status` | `started` |
| `rc-service chronyd status` | `started` |
| `ip addr show end1` | 已取得 `192.168.1.0/24` 网段内的地址，通常为 `192.168.1.157` |
| `ping -c 3 192.168.1.2` | 三次均收到回复 |
| `df -h /` | 根文件系统使用率正常 |

`chronyd` 在无外部时间源时不会完成同步，服务处于运行状态即可。

## 7. 回滚

### 7.1 无法正常启动

按第 4 章的方法重新以 `init=/bin/sh` 启动。分区与内核未改动。

### 7.2 配置损坏

`install.sh` 在改动之前把 `/etc` 打包到 `/root/etc-backup.tar.gz`。恢复：

```sh
mount -o remount,rw /
tar xzf /root/etc-backup.tar.gz -C /
sync
reboot -f
```

### 7.3 撤销软件包

需要移除本次安装的软件包时，按第 5 章的列表执行 `apk del`。即使软件包移除不完整，仍可通过 `init=/bin/sh` 进入系统。

## 8. 已知限制

- 串口输入仍不可靠：行首字符丢失与粘贴过程中的字符丢失属于链路问题，安装 OpenRC 不会改变这一情况。处理方法见《用 ostool 操作 K3 开发板》5.4 节。
- 开发板仍无外网：后续安装软件同样需要准备离线包。
- 根文件系统仍保留容器时期的文件：`/etc/fstab` 仍是厂商提供的版本，`/proc`、`/sys`、`/dev` 由 OpenRC 自行挂载。
- 基础软件包来自 3.23.7，与原有的 3.23.4 属于同一分支；`/etc/os-release` 中的版本号可能仍显示 3.23.4。
- SSH 服务已启用，但宿主机没有通往实验网络的路由，仍只能通过串口操作。原因见《用 ostool 操作 K3 开发板》第 1 章。
