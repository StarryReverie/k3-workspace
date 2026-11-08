# 把 K3 上的 Alpine 变成可正常使用的 Linux：方案汇总

日期：2026-11-02  
适用范围：BananaPi BPI-SM10（SpacemiT K3），BananaPi-1 / Alpine 3.23.4  
相关文档：[`04-k3-findings.md`](04-k3-findings.md)、[`03-pitfalls.md`](../03-pitfalls.md)

本文记录“把板子上残缺的系统修成一个能正常启动、能联网、有正规 init 的 Linux”的所有候选方案，以及当前推荐路线。**方案 A 是下一步要执行的方案，其余方案保留作为备选或长期方向。**

---

## 0. 现状与结论（2026-11-02 实测）

### 0.1 硬件 / 启动链

| 项目 | 实测值 |
|---|---|
| 板卡 | BananaPi-1（Alpine） |
| SoC | SpacemiT K3，rv64imafdcvh |
| 内核 | `6.18.3-generic #1.0.0.4` |
| 根分区 | `/dev/sda3`，ext4，116.9 G，已用 681.7 M |
| 启动方式 | U-Boot → vendor 内核 / DTB / initramfs → `switch_root` |
| 串口 | `ttyS0,115200`，无硬件流控 |

### 0.2 系统现状

```text
NAME="Alpine Linux"
ID=alpine
VERSION_ID=3.23.4
```

```text
/sbin/init -> /bin/busybox
/bin/sh    -> /bin/busybox
/sbin/apk                 存在（apk-tools 3.0.6-r0）
/etc/apk/keys             正常（alpine-devel 两个公钥）
/etc/init.d               不存在
/lib/modules              不存在
/etc/fstab                只有 cdrom / usbdisk，没有 root/proc/sys/dev
```

`/etc/apk/world`：

```text
alpine-baselayout
alpine-keys
alpine-release
apk-tools
binutils
busybox
gcc
git
libusb-dev
musl-dev
musl-utils
vim
```

### 0.3 关键判断

这是一个**按“容器 rootfs”铺到 sda3 上的 Alpine**，不是完整的可启动系统：

- `alpine-base` 的启动层（`openrc`、`busybox-openrc`、`busybox-mdev-openrc`）没有安装
- `/etc/inittab` 却是 Alpine 默认的 OpenRC 版本
- 所以正常启动会连续报三次：

  ```text
  can't run '/sbin/openrc': No such file or directory
  ```

- `ttyS0` 的 getty 在 inittab 里是注释状态，root 又是锁定状态
- 结果：正常启动没有串口 shell，只能 `init=/bin/sh`

`world` 里已经有 `gcc / git / vim / binutils / musl-dev`，说明它更像一个“开发用容器镜像”，只是被直接当成了根文件系统。

### 0.4 网络实测

```text
ip link set end1 up          # 4 秒后 Link is Up - 1Gbps/Full
udhcpc -i end1               # 成功拿到 192.168.1.157/24，server 192.168.1.2
ip route                     # 只有 192.168.1.0/24，没有 default route
cat /etc/resolv.conf         # QEMU SLIRP 残留（nameserver 10.0.2.3）
ping 192.168.1.2             # 通
wget http://192.168.1.2:2999/  # 通
wget https://dl-cdn.alpinelinux.org/...  # 失败：bad address（DNS）
```

结论：

- 实验网 `192.168.1.0/24` 内可用，`192.168.1.2:2999` 的会话 HTTP 共享可达
- **板子没有默认路由，也没有可用 DNS，实际上没有外网**
- 因此安装包必须提前在宿主机准备好，走会话共享上传

> 注意：`udhcpc` 没写路由/DNS，是因为 rootfs 里缺 `/usr/share/udhcpc/default.script` 之类的配置。方案 A 会用 `dhcpcd` 接管，不依赖这个脚本。

### 0.5 已确认的产品决策

| 决策 | 值 |
|---|---|
| root 登录方式 | 密码登录（密码由用户指定，**不写入仓库**） |
| 网络访问通道 | 只能串口；不做 Tailscale / 反向隧道 |
| 第一版范围 | 最小可用系统：OpenRC + getty + DHCP + sshd + chrony |
| SSH 策略 | 建议 `PermitRootLogin prohibit-password`（只允许密钥）；如需密码登录再显式打开 |

---

## 1. 方案 A：原地补装 OpenRC（推荐）

### 1.1 思路

保留现有 rootfs 的内核兼容层和已装工具，只把缺失的“启动层 + 服务”补上：

- 装 `alpine-base`、`openrc`、各服务的 `-openrc` 子包
- 写 `inittab`，启用 `ttyS0` getty
- 配 `dhcpcd`、`sshd`、`chrony`
- 设置 root 密码、SSH 密钥
- 加入标准 runlevel

**不动**：分区表、PARTUUID、`/boot`、内核、DTB、initramfs、`/lib/modules`、现有 `gcc/git/vim`。

### 1.2 为什么可行

- `apk-tools 3.0.6` 和 `/etc/apk/keys` 都正常，可以离线安装本地包
- 板子已在 `sda3` 上，空间 110 G 可用
- U-Boot 启动链和 vendor initramfs 都是好的，`switch_root` 后执行 `/sbin/init`（BusyBox）即可
- 任何失败仍可用 `init=/bin/sh` 进入，不会锁死

### 1.3 离线包来源

Alpine 官方 U-Boot 发布包自带一套离线仓库：

```text
https://dl-cdn.alpinelinux.org/alpine/v3.23/releases/riscv64/alpine-uboot-3.23.7-riscv64.tar.gz
```

- 整个包 51 MB，其中 `apks/riscv64/` 约 17 MB，103 个包
- 版本 3.23.7，同分支小版本，可安全覆盖 3.23.4 的 base 包
- 已包含：

  ```text
  openrc openrc-user ifupdown-ng
  busybox-openrc busybox-mdev-openrc
  openssh openssh-server-common-openrc openssh-keygen
  dhcpcd dhcpcd-openrc
  chrony chrony-openrc openntpd openntpd-openrc
  doas e2fsprogs e2fsprogs-extra
  ca-certificates-bundle tzdata wget
  ```

需要额外补的（不在离线仓库里）：

```text
dropbear（可选，替代 openssh）
bash / nano / htop / tmux / python3（可选）
tailscale（本方案不装）
```

### 1.4 安装包清单（最小集）

```text
alpine-base
openrc
ifupdown-ng            # 满足 openrc 的 ifupdown-any 依赖
busybox-openrc
busybox-mdev-openrc
openssh
openssh-server-common-openrc
openssh-keygen
dhcpcd
dhcpcd-openrc
chrony
chrony-openrc
e2fsprogs
e2fsprogs-extra
doas
ca-certificates-bundle
tzdata
wget
```

### 1.5 配置

`/etc/inittab`：

```text
# /etc/inittab

::sysinit:/bin/mount -o remount,rw /
::sysinit:/bin/dmesg -n 1
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
::wait:/sbin/openrc default

tty1::respawn:/sbin/getty 38400 tty1
tty2::respawn:/sbin/getty 38400 tty2
tty3::respawn:/sbin/getty 38400 tty3
tty4::respawn:/sbin/getty 38400 tty4
tty5::respawn:/sbin/getty 38400 tty5
tty6::respawn:/sbin/getty 38400 tty6

ttyS0::respawn:/sbin/getty -L 115200 ttyS0 vt100

::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
```

说明：

- `mount -o remount,rw /`：vendor initramfs 结束时会 remount 成只读
- `dmesg -n 1`：关掉触摸屏驱动的刷屏日志，避免插进串口输入
- `ttyS0` 使用 BusyBox getty，`-L` 表示 local line，不等待载波

标准 runlevel（服务存在才加）：

```text
sysinit : devfs dmesg hwdrivers mdev modules
boot    : bootmisc hostname hwclock root swap sysctl syslog urandom
default : dhcpcd sshd chronyd crond
shutdown: killprocs mount-ro savecache
```

其他配置：

```text
/etc/hostname                k3-alpine
/etc/hosts                   127.0.0.1 localhost k3-alpine
/etc/conf.d/dhcpcd           只处理 end1
/etc/conf.d/chronyd          makestep
/etc/ssh/sshd_config         PermitRootLogin prohibit-password
/root/.ssh/authorized_keys   用户公钥（可选）
/etc/doas.conf               可选
```

root 密码：

- 用户已确认使用密码登录
- 部署脚本从环境变量或临时文件读取密码，在宿主机生成 `$6$` 哈希后直接写 `/etc/shadow`
- **密码本身不写入仓库、不写入文档**
- 不走串口交互式 `passwd`，避免输入掉字把密码写坏

### 1.6 执行阶段

#### Phase 0：侦察（已完成 2026-11-02）

已确认：Alpine 3.23.4、apk 可用、无 `/etc/init.d`、无 `/lib/modules`、DHCP 可用但无默认路由、`192.168.1.2:2999` 可达、无外网。  
`/etc` 已备份到板子上的 `/root/etc-backup.tar.gz`（167,279 字节）。

#### Phase 1：宿主机准备离线包

1. 下载 `alpine-uboot-3.23.7-riscv64.tar.gz` 并校验 sha256
2. 取出 `apks/riscv64/`
3. 加入配置文件和 `install.sh`
4. 打成一个 ≤64 MiB 的 tar.gz（预计 18–20 MB）

#### Phase 2：安装会话

U-Boot：

```text
setenv commonargs 'setenv bootargs splash init=/bin/sh rw'
run bootcmd
```

板子上：

```sh
mount -o remount,rw /
ip link set end1 up
udhcpc -i end1 -n -q -t 3 -T 2
wget -O /root/k3-offline.tar.gz http://192.168.1.2:2999/share/sessions/<SID>/boot/k3-offline.tar.gz
mkdir -p /root/offline && tar xzf /root/k3-offline.tar.gz -C /root/offline
apk add --repository /root/offline/apks \
  alpine-base openrc ifupdown-ng busybox-openrc busybox-mdev-openrc \
  openssh openssh-server-common-openrc openssh-keygen \
  dhcpcd dhcpcd-openrc chrony chrony-openrc \
  e2fsprogs e2fsprogs-extra doas ca-certificates-bundle tzdata wget
```

然后写配置、设置密码、`rc-update add`、`sync`。

#### Phase 3：正常启动验证

重新 `board connect`，**不打断 autoboot**：

```text
OpenRC 启动日志
→ k3 login:
```

登录后检查：

```sh
rc-status
ip addr show end1
ping -c3 192.168.1.2
ssh root@127.0.0.1 true
chronyc tracking
df -h /
```

### 1.7 串口手感

因为只能走串口，方案 A 额外保证：

1. 启动早期 `dmesg -n 1`，减少日志插入
2. 根文件系统显式 remount rw
3. `getty -L 115200 ttyS0 vt100` 标准登录
4. 如果仍掉字，只对 `ttyS0` 的登录 shell 设 `TERM=dumb`，不影响 SSH
5. 不接受 `stty -echo` 残留；脚本结束必须 `stty echo icanon`

### 1.8 回滚

| 场景 | 处理 |
|---|---|
| 安装后无法正常启动 | U-Boot 里 `commonargs ... init=/bin/sh` 进入 |
| 配置写坏 | `/root/etc-backup.tar.gz` 覆盖 `/etc` |
| 包冲突 | `apk del` 新装包，或从备份恢复 |
| 最坏情况 | 仍可用 `init=/bin/sh`，分区和内核未动 |

### 1.9 优点 / 缺点

**优点**

- 改动最小，启动链和硬件驱动完全不动
- 离线可完成，不依赖板子外网
- 失败可恢复
- 传输量小（约 20 MB）
- 保留已有的 `gcc/git/vim` 开发环境

**缺点**

- rootfs 里仍会留有一些“容器镜像”的历史文件
- 版本会从 3.23.4 小幅升到 3.23.7
- `/etc/fstab` 仍是 vendor 风格，需要 OpenRC 自己挂 `/proc /sys /dev`

### 1.10 已实现的离线 bundle

宿主机构建：

```bash
K3_ROOT_PASSWORD='<root 密码>' ./scripts/build-offline-bundle.sh
```

产物：

```text
dist/k3-offline.tar.gz
```

当前构建结果（2026-11-03）：

```text
size    16 MiB (16,791,159 bytes)
sha256  ebf132edf66ca96442de6fd6b2f49f4f9bfb58ef589cd7e6b86fb361b3f78d6b
```

bundle 内容：

```text
offline/install.sh
offline/root.hash
offline/README.txt
offline/apks/riscv64/APKINDEX.tar.gz
offline/apks/riscv64/*.apk        # 102 个包，约 17 MB
```

用法：

```bash
# 宿主机：上传到会话，远端路径越短越好
source scripts/session-env.sh logs/session-<时间戳>.log
./scripts/upload.sh dist/k3-offline.tar.gz boot/k.tar.gz

# 板子（init=/bin/sh）
stty echo icanon
dmesg -n 1
wget -O /root/k.tar.gz 'http://192.168.1.2:2999/share/sessions/<SID>/boot/k.tar.gz'
tar xzf /root/k.tar.gz -C /root
sh /root/offline/install.sh
sync
reboot -f
```

说明：

- `install.sh` 会先检查 `ID=alpine`，误分配到 Yocto 板时直接退出
- 本地仓库会写进 `/etc/apk/repositories`，原有镜像地址保留为注释
- root 密码只以 `$6$` 哈希形式出现在 bundle 的 `root.hash` 里
- `dist/` 已在 `.gitignore` 中，不提交二进制产物

---

## 2. 方案 B：用官方 Alpine rootfs 覆盖 sda3

### 2.1 思路

把 `sda3` 上的内容整体替换成官方 Alpine，再装 OpenRC 和服务。

可选来源：

| 来源 | 大小 | 说明 |
|---|---|---|
| `alpine-minirootfs-3.23.7-riscv64.tar.gz` | 3.4 MB | 容器 rootfs，**不含 openrc**，装完还要补 |
| `alpine-standard` ISO | 55 MB | 完整安装介质，不适合直接铺盘 |
| `alpine-uboot` tar.gz | 51 MB | 含 U-Boot、extlinux、离线 apk 仓库；只取 `apks/` 更有用 |

### 2.2 流程要点

1. 不要 `mkfs`：保留原分区和 PARTUUID，避免动 vendor initramfs 的 `root=PARTUUID=`
2. 先把新 rootfs 解到 `/newroot`，校验 `/newroot/sbin/init`、`/newroot/etc/inittab`
3. 覆盖到 `/` 时不要用正在运行的 `/bin/busybox` 去覆盖它自己：
   - 用 `/newroot/bin/busybox cp -a /newroot/. /`，或
   - 从 initramfs shell（`rdinit=/bin/sh`）里操作
4. `sync` 后重启

### 2.3 风险

- 覆盖过程中如果中断，可能留下半套 rootfs
- 需要自己处理 `/etc/fstab`、`/etc/apk/world`、网络配置
- 虽然实测 `/lib/modules` 不存在（驱动内建或 initramfs 提供），但覆盖前仍应确认
- 官方 minirootfs 依然要再装 OpenRC，收益不如方案 A

### 2.4 结论

**不作为首选。** 只有当前 rootfs 被证明有无法修复的损坏时才考虑。

---

## 3. 方案 C：Debian + systemd

### 3.1 思路

用 Debian riscv64 的 rootfs（或 debootstrap 产物）替换 sda3，得到 systemd、apt、完整用户空间。

### 3.2 可行性

- Debian 官方提供 riscv64 的 rootfs / netboot 包
- vendor 内核 6.18.3 支持标准 syscall，systemd 可以跑
- vendor initramfs 只做“挂载 rootfs + switch_root”，理论上兼容 Debian

### 3.3 代价

| 项目 | 估计 |
|---|---|
| rootfs 压缩包 | 150–250 MB |
| 解压后 | 600 MB–1 GB |
| 传输 | 超过 64 MiB 上限，需要分块上传和重组 |
| 首次启动 | 需要网络配置、SSH、串口 getty 全部重做 |
| 与 vendor initramfs 的兼容性 | 需要实测，出问题排查成本高 |

### 3.4 结论

**现阶段不做。** 如果以后明确需要 systemd / apt 生态，再作为独立项目评估。

---

## 4. 方案 D：NixOS

### 4.1 思路

用 Nix 构建 riscv64 NixOS 系统，铺到 sda3 或 SD 卡。

### 4.2 为什么现在不建议

- riscv64 没有官方二进制缓存，需要交叉编译或 QEMU 模拟构建
- 整个 systemd + 用户空间的 closure 通常 >1 GB
- 传输只能走 64 MiB 限制的会话共享，需要长时间分块
- 需要自建或匹配内核、initrd、设备树，调试成本高
- 一旦启动失败，恢复仍然是串口 + U-Boot

### 4.3 什么时候值得做

- 平台提供 riscv64 原生构建机或二进制缓存
- 或者能直接把板子挂到 Nix remote builder
- 并且接受“先做几小时构建、再传输几 GB”的成本

### 4.4 结论

**长期可选项，不作为当前修复方案。** 当前用 Nix 只做“宿主机侧的可复现构建脚本”，不把 NixOS 铺到板子上。

---

## 5. 方案 E：自定义 initramfs + `rdinit`

### 5.1 定位

这不是一个“完整 OS”，而是两种用途：

1. **跑自己的程序**：把静态 RISC-V 二进制放进 initramfs，`rdinit=/your-program`，完全不依赖 Alpine/Yocto rootfs
2. **安装 / 救援**：initramfs 里带静态 busybox、离线包、安装脚本，用来写盘或修复 rootfs

### 5.2 做法

- 用 `busybox-static`（Alpine 包）或自建静态 busybox
- `cpio` 打包成 `initramfs.cpio.gz`
- 上传到会话共享，U-Boot 里：

  ```text
  tftpboot <ramdisk_addr> <initramfs>
  ```

- 配合 vendor 内核 `booti`，或直接 `rdinit=/install.sh`

### 5.3 适用场景

- 板子完全无法从 rootfs 启动
- 需要在“目标 rootfs 未被挂载”的状态下安全覆盖文件
- 只想验证 Rust 程序能不能在 K3 上跑

### 5.4 结论

**保留为救援和实验手段**，不作为日常系统方案。

---

## 6. 方案 F：不动 Alpine，直接用 Yocto 板（BananaPi-2）

### 6.1 现状

Yocto/Poky 5.3.99 + systemd 已经是完整系统：

- `serial-getty@ttyS0.service`
- `sshd.socket`
- systemd 网络管理
- 正常启动到 `k3 login:`

### 6.2 问题

- 登录凭据未知（root 密码 / 是否允许空密码未确认）
- `board connect` 自动分配，可能这次是 Alpine、下次是 Yocto
- `--board-id` 不可靠

### 6.3 结论

- **不改 Yocto 板**
- 如果能确认 Yocto 的登录方式，它本身就是一个可用的 Linux
- 若平台能固定租约到某一块板，体验会稳定很多

---

## 7. 方案对比

| 方案 | 改动量 | 传输量 | 风险 | 得到什么 | 结论 |
|---|---|---|---|---|---|
| A. 原地补 OpenRC | 小 | ~20 MB | 低 | 标准 Alpine + OpenRC + 网络 + SSH | **推荐，先做** |
| B. 官方 Alpine 覆盖 | 中 | ~20 MB | 中 | 更干净的 Alpine | 备选 |
| C. Debian + systemd | 大 | 150 MB+ | 高 | systemd / apt | 暂不做 |
| D. NixOS | 很大 | GB 级 | 很高 | 声明式系统 | 长期 |
| E. 自定义 initramfs | 小 | <20 MB | 低 | 跑单程序 / 救援 | 辅助 |
| F. 用 Yocto 板 | 无 | 0 | 低 | 现成完整系统 | 只差凭据 |

---

## 8. 推荐执行顺序

```text
1. 方案 A：把 BananaPi-1 修成标准 Alpine
2. 方案 E：保留一个 initramfs 救援/安装镜像
3. 方案 F：确认 Yocto 登录方式，避免被分配到它时抓瞎
4. 方案 B：只有 A 失败且 rootfs 确实损坏时才做
5. 方案 C / D：作为长期方向，不在当前阶段投入
```

---

## 9. 方案 A 的验收标准

全部满足才算完成：

- [ ] 不按 Ctrl+C，正常启动能到 `k3 login:`
- [ ] root 密码登录成功
- [ ] `rc-status` 显示 sshd / dhcpcd / chronyd 已启动
- [ ] `end1` 自动拿到 `192.168.1.x`
- [ ] `ping 192.168.1.2` 成功
- [ ] `ssh root@127.0.0.1 true` 成功
- [ ] 串口连续输入 10 个字符不掉字
- [ ] 重启两次后配置仍然生效
- [ ] `/etc/inittab` 不再出现 `can't run '/sbin/openrc'`

---

## 10. 明确不做的事

- 不修改 Yocto 板（BananaPi-2）
- 不对任何分区执行 `mkfs`
- 不写分区表
- 不执行 `saveenv`
- 不把 root 密码、token、私钥写入仓库
- 不在未确认的情况下覆盖 rootfs

---

## 11. 参考

- [`04-k3-findings.md`](04-k3-findings.md)：双板镜像、`commonargs` 覆盖、串口问题、Rust 交叉编译
- [`03-pitfalls.md`](../03-pitfalls.md)：坑位表
- Alpine 3.23 发布包：<https://dl-cdn.alpinelinux.org/alpine/v3.23/releases/riscv64/>
- Alpine OpenRC：<https://wiki.alpinelinux.org/wiki/OpenRC>
