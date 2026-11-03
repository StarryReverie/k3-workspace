# board

板端脚本与离线安装说明。

## 内容

| 文件 | 用途 |
|---|---|
| `install.sh` | 在现有 Alpine rootfs 上补装 OpenRC + getty + DHCP + sshd + chrony |

`install.sh` 会：

- 检查 `ID=alpine`，不是 Alpine 直接退出
- 用 `/root/offline/apks` 作为本地 APK 仓库
- 安装 `alpine-base`、`openrc`、OpenSSH、dhcpcd、chrony 等
- 写 `/etc/inittab`，启用 `ttyS0` getty
- 设置 root 密码哈希（从 `root.hash` 读取）
- 配置 runlevel、SSH、主机名
- `sync`

不会：

- `mkfs` / 写分区表 / 改 PARTUUID
- 动内核、DTB、initramfs
- 改 U-Boot 环境
- 碰 Yocto 板

## 构建 bundle

在仓库根目录：

```bash
K3_ROOT_PASSWORD='<root 密码>' ./scripts/build-offline-bundle.sh
```

产物：

```text
dist/k3-offline.tar.gz
```

可选：

```bash
K3_AUTHORIZED_KEYS=~/.ssh/id_ed25519.pub \
K3_ROOT_PASSWORD='<root 密码>' \
./scripts/build-offline-bundle.sh
```

## 在板子上执行

U-Boot：

```text
setenv commonargs 'setenv bootargs splash init=/bin/sh rw'
run bootcmd
```

板子 shell：

```sh
stty echo icanon
dmesg -n 1
wget -O /root/k.tar.gz 'http://192.168.1.2:2999/share/sessions/<SID>/boot/k.tar.gz'
tar xzf /root/k.tar.gz -C /root
sh /root/offline/install.sh
sync
reboot -f
```

重新连接后不要打断 autoboot，等待 `k3 login:`。

## 验证

```sh
rc-status
ip addr show end1
ping -c3 192.168.1.2
ssh root@127.0.0.1 true
chronyc tracking
```

安装日志：`/root/k3-install.log`  
`/etc` 备份：`/root/etc-backup.tar.gz`
