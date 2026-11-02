# K3 实测发现：双板镜像、Alpine rootfs、串口与 Rust 交叉编译

日期：2026-11-02  
环境：BananaPi BPI-SM10（SpacemiT K3），ostool 0.31.1  
说明：本文只记录**实际验证过**的现象和命令，避免下次重新踩坑。

## 0. 一句话总结

平台至少有两块 BananaPi，镜像完全不同：一块是 **Alpine**，一块是 **Yocto/Poky + systemd**。`ostool board connect -b BananaPi` 由服务端自动分配，`--board-id` 经常被拒绝，所以同一个“BananaPi”在不同会话里会表现出完全不同的启动日志、登录方式和 shell 行为。

Alpine 镜像没有安装 OpenRC，但 `/etc/inittab` 仍然是 Alpine 默认的 OpenRC 配置；正常启动会连续报 3 次 `can't run '/sbin/openrc'`，并且没有 `ttyS0` shell。要进系统目前最稳的方式仍然是 U-Boot 里覆盖 `commonargs`，用 `init=/bin/sh` 启动。

---

## 1. 两块板、两套镜像

| 板子 | rootfs | 串口表现 |
|---|---|---|
| BananaPi-1 | Alpine Linux 3.23.4 | 原始配置下无串口 shell；`init=/bin/sh` 可进 root shell |
| BananaPi-2 | Yocto/Poky 5.3.99（systemd） | `k3 login:`，有 serial-getty@ttyS0、sshd |

实测证据：

- BananaPi-1 / Alpine：
  - `/etc/os-release` 为 `ID=alpine`、`VERSION_ID=3.23.4`
  - `/dev/sda3` ext4，UUID `07ff5c54-8023-49a0-bb5d-cc09b337e44e`
  - 文件数约 4274，文件系统 31,132,639 个 4K block（约 119 GiB）
- BananaPi-2 / Yocto：
  - `Poky (Yocto Project Reference Distro) 5.3.99+snapshot-...`
  - `/dev/sda3` ext4，UUID `c722fc59-3bc2-4827-8066-3917e86b13c1`
  - 文件数约 252,672，文件系统约 1 GiB
  - systemd 启动日志、`serial-getty@ttyS0.service`、`sshd.socket`

存储：

- UFS：`KINGSTON TY7B-128`，128 GB，内核里是 `sda`，有 `sda1 sda2 sda3`
- SD：`mmc0:aaaa SK32G 29.7 GiB`，内核里是 `mmcblk0`，只有 `p1`
- 两套镜像都可能在 UFS 的 `sda3` 上；具体是哪套取决于当前分配到哪块板

板卡分配：

```text
ostool board ls
# 只显示当前可用的那一块，但 connect 时仍可能分到另一块
```

```text
ostool board connect -b BananaPi --board-id BananaPi-1
# 实测报错：
# server allocated board `BananaPi-2` while board `BananaPi-1` was requested
```

结论：不要依赖 `--board-id`，也不要假设“这次连的还是上次那块板”。

---

## 2. Alpine rootfs 的实际情况

BananaPi-1 上的关键事实：

```text
/sbin/init -> /bin/busybox
/bin/sh    -> /bin/busybox
/sbin/openrc        不存在
/etc/init.d         不存在
/etc/shadow root:*  root 被锁定
```

原始 `/etc/inittab`（570 字节）：

```text
# /etc/inittab

::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
::wait:/sbin/openrc default

# Set up a couple of getty's
tty1::respawn:/sbin/getty 38400 tty1
tty2::respawn:/sbin/getty 38400 tty2
tty3::respawn:/sbin/getty 38400 tty3
tty4::respawn:/sbin/getty 38400 tty4
tty5::respawn:/sbin/getty 38400 tty5
tty6::respawn:/sbin/getty 38400 tty6

# Put a getty on the serial port
#ttyS0::respawn:/sbin/getty -L 115200 ttyS0 vt100

# Stuff to do for the 3-finger salute
::ctrlaltdel:/sbin/reboot

# Stuff to do before rebooting
::shutdown:/sbin/openrc shutdown
```

正常启动流程：

1. initramfs 挂载 `/dev/sda3`，修改 `/etc/fstab`，再 remount 为只读
2. `switch_root` 后 BusyBox init 读取上面的 inittab
3. 三条 OpenRC 入口全部 `ENOENT`：

```text
can't run '/sbin/openrc': No such file or directory   (x3)
```

4. `ttyS0` 的 getty 被注释掉，root 又是锁定状态，所以串口没有 shell

这不是 OpenRC 崩了，而是**镜像里根本没装 OpenRC**。

---

## 3. 用 `init=/bin/sh` 进 Alpine

### 3.1 关键坑：直接改 `bootargs` 会被覆盖

U-Boot 环境实测：

```text
bootcmd=run autoboot; echo "run autoboot"
commonargs=setenv bootargs splash
bootdelay=3
loglevel=8
bootargs          # 未定义
```

启动链是：

```text
autoboot
  -> commonargs          # setenv bootargs splash
  -> add_bootarg         # 追加 console/loglevel
  -> set_nor_args / set_mmc_args
  -> boot_kernel
       -> set_root_arg   # 追加 root=PARTUUID=... bootfs=PARTUUID=...
       -> detect_dtb
       -> loadknl / loaddtb / loadramdisk
       -> start_kernel
```

所以：

```text
setenv bootargs ${bootargs} init=/bin/sh rw
run bootcmd
```

**不会生效**，因为 `commonargs` 在开头把 `bootargs` 重设成了 `splash`。

正确做法是把参数注入 `commonargs`：

```text
setenv commonargs 'setenv bootargs splash init=/bin/sh rw'
run bootcmd
```

实测内核命令行会出现：

```text
splash init=/bin/sh rw loglevel=8 ... root=PARTUUID=... ...
```

然后进入 rootfs 的 `/bin/sh`（会打印 `can't access tty; job control turned off`，正常）。

### 3.2 `init=` 与 `rdinit=` 的区别

| 参数 | 运行位置 |
|---|---|
| `init=/bin/sh` | `switch_root` 之后，rootfs 里的 `/bin/sh` |
| `rdinit=/bin/sh` | initramfs 里的 `/bin/sh`，不挂 rootfs |

要检查 Alpine rootfs，用 `init=/bin/sh`；要绕过 rootfs，用 `rdinit=...`。

---

## 4. 串口日志干扰与输入掉字

### 4.1 触摸屏日志

启动时 `raspits-ft5426` 会每秒报一次：

```text
raspits-ft5426: fts_i2c_read: i2c read issue, please check touchscreen connection, -6
raspits-ft5426: raspits_ft5426_work: checking touch ic, countdown: 7
...
raspits-ft5426: raspits_ft5426_work: checking touch ic timeout, -6
```

持续约 11 秒，然后停止。它和 OpenRC、输入掉字都不是同一个问题；只是内核 console 和 shell 共用 `ttyS0`，所以会插入终端。

### 4.2 降低 console 日志

`loglevel=8` 来自 U-Boot。进入系统后可以：

```sh
echo 1 > /proc/sys/kernel/printk
# 或
dmesg -n 1
```

实测：

```text
console loglevel: 1	4	1	7
```

但**启动早期**的触摸屏日志已经打出来了；`printk=1` 只能让之后的日志安静。若希望 shell 一出现就干净，可以在 inittab 里加 `sleep 15`，等日志刷完再起 shell。

### 4.3 输入掉字的真正原因

BusyBox 的交互式 ash 会使用行编辑，并发送：

```text
ESC[6n
```

询问终端光标位置。`ostool` 的串口终端/我们的 Python pty 不是完整终端模拟器，响应不正确时行编辑状态会吃掉或打乱输入。实测在日志大量输出时更严重。

另外，脚本里执行过：

```sh
stty -echo
```

这个 tty 设置会**保留**。如果后续 shell 没有重置，就会表现为“能输入但看不到回显”。

教训：

- 脚本化操作结束后，恢复：

  ```sh
  stty echo icanon
  ```

- 如果要做可靠的串口自动化，不要让交互式 ash 行编辑参与；可以用一个简单的 `read` 循环，或直接用 `init=/bin/sh`（无 job control，行编辑通常关闭）。
- 等待启动日志停止后再输入，或降低 console loglevel。
- 真实终端模拟器通常会正确回应 `ESC[6n`，所以人工连接可能比 pty 脚本更稳。

> 2026-11-02 曾尝试过“`printk=1` + `sleep 15` + 自定义 `k3-shell` read 循环 + `stty echo icanon`”，但用户要求恢复原始 inittab，因此这些修改已撤回；仅保留本文作为经验记录。

---

## 5. Rust 交叉编译（rust-overlay，无 pkgsCross）

### 5.1 flake 配置

```nix
inputs.rust-overlay = {
  url = "github:oxalica/rust-overlay";
  inputs.nixpkgs.follows = "nixpkgs";
};

# pkgs 需要应用 overlay
pkgs = import nixpkgs {
  inherit system;
  overlays = [ (import rust-overlay) ];
};

rust = pkgs.rust-bin.stable.latest.default.override {
  targets = [ "riscv64gc-unknown-linux-musl" ];
};
```

不需要 `pkgsCross`。Rust 的 musl target 自带 self-contained CRT 和 `libc.a`，配合工具链里的 `rust-lld` 就能静态链接。

### 5.2 `.cargo/config.toml`

必须放在**运行 cargo 的目录**（本工作区是仓库根目录），否则 `--manifest-path` 不会读取 crate 内的 `.cargo/config.toml`：

```toml
[target.riscv64gc-unknown-linux-musl]
linker = "rust-lld"
rustflags = [
  "-C", "link-self-contained=yes",
  "-C", "target-feature=+crt-static",
]
```

`rust-lld` 不在默认 PATH，需要在 devShell 里加：

```sh
export PATH="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin:$PATH"
```

### 5.3 构建

```bash
cargo build --manifest-path rust-hello/Cargo.toml --release \
  --target riscv64gc-unknown-linux-musl
```

产物：

```text
rust-hello/target/riscv64gc-unknown-linux-musl/release/rust-hello
```

验证：

```text
ELF 64-bit LSB executable, UCB RISC-V, RVC, double-float ABI,
statically linked, not stripped
```

程序输出标记：

```text
hello from Rust on K3
arch = riscv64
os   = linux
RUST_K3_OK
```

注意：**二进制已编译成功，但尚未在板上实际执行成功**。下一步最稳的是把它放进自定义 initramfs，用 `rdinit=/rust-hello` 启动，完全不依赖 Alpine/Yocto rootfs。

---

## 6. 板子 Linux 网络与文件传输

### 6.1 BusyBox applet

Alpine 的 BusyBox 有：

```text
ip
ping
nc
wget
```

没有：

```text
tftp
ftpget
```

所以“在板子 Linux 里用 tftp 下载”这条路走不通；U-Boot 的 `tftpboot` 仍然可用。

### 6.2 网卡

接口名：

```text
end1
```

驱动：

```text
dwmac-spacemit-ethqos cac82000.ethernet
```

`ip link set end1 up` 后约 4 秒出现：

```text
Link is Up - 1Gbps/Full - flow control off
```

不要一执行 `ip link set` 就立刻 ping；等 `Link is Up` 再测。

常用配置：

```sh
ip link set end1 up
ip addr add 192.168.1.157/24 dev end1
ping -c 1 192.168.1.2
```

### 6.3 HTTP 共享下载（待验证）

ostool 会话文件除了 `tftp_url`，API 文档还提供 HTTP 共享路径：

```text
http://192.168.1.2:2999/share/sessions/<SID>/boot/<file>
```

板子上有 `wget`，可以尝试：

```sh
busybox wget -O /tmp/rust-hello \
  http://192.168.1.2:2999/share/sessions/<SID>/boot/rust-hello

chmod +x /tmp/rust-hello
/tmp/rust-hello
```

注意：`/run` 挂载带 `noexec`，不要放 `/run` 执行；用 `/tmp` 或 rw 的 rootfs。

### 6.4 会话上传

```bash
source scripts/session-env.sh logs/session-<timestamp>.log
./scripts/upload.sh rust-hello boot/rust-hello
```

- 单文件上限约 64 MiB
- 会话约 5 分钟回收
- `upload.sh` 返回 `tftp_url`；`http_url` 可能为空
- `board connect` 后 token 可能刷新，上传前重新读取 keyring / 重新 `source`

---

## 7. ostool 会话与 SID

`ostool board connect` 启动时会打印：

```text
Allocated board session:
  board_type: BananaPi
  board_id: BananaPi-1
  session_id: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
  lease_expires_at: ...
```

默认**不会**把这段写入文件。

- 想留日志：用 `./scripts/capture-session.sh`，生成 `logs/session-<时间戳>.log`
- 提取 SID：

  ```bash
  source scripts/session-env.sh logs/session-<时间戳>.log
  echo "$SID"
  ```

- 如果直接 `ostool board connect`，没有日志，只能从终端回滚里复制 `session_id`，或者手动：

  ```bash
  source scripts/session-env.sh
  export SID='<粘贴 session_id>'
  ```

  `session-env.sh` 在没有日志时会把 `SID` 置空，所以要在 `source` 之后覆盖。

- token 在 keyring；上传 401 时先 `ostool board ls` 刷新，再重新 `source`。

---

## 8. 当前恢复状态（2026-11-02）

用户要求恢复原始 inittab，已完成：

```text
/etc/inittab      570 字节  原始内容
/etc/inittab.bak  570 字节  原始内容
/etc/inittab.k3.bak       最后一次修改版（581 字节）
```

- `/usr/local/bin/k3-shell`、`/usr/local/bin/k3-quiet.sh` 已删除
- 已 `sync`
- `stty echo icanon` 已在恢复会话末尾执行

`/etc/profile` 仍有残留（919 字节），包含之前追加的：

```text
# K3 serial console: avoid BusyBox ash line editing and cursor queries.
export TERM=dumb
export COLUMNS=80
export LINES=24
```

清理：

```sh
sed -i '/^# K3 serial console:/,/^export LINES=24$/d' /etc/profile
```

恢复后的行为：

- 正常启动回到原始 Alpine：OpenRC 三连报错、无串口 shell
- 手动进 shell 仍然用：

  ```text
  setenv commonargs 'setenv bootargs splash init=/bin/sh rw'
  run bootcmd
  ```

- 如果没有回显：

  ```sh
  stty echo icanon
  ```

---

## 9. 建议

1. **跑自己的程序**：优先用自定义 initramfs + `rdinit=/your-program`，不要依赖 Alpine/Yocto rootfs。两块板的内核都是 6.18.3，initramfs 方案不受镜像切换影响。
2. **交互使用**：继续用 `init=/bin/sh`；不要默认依赖正常启动的 Alpine，因为它没有串口 getty 且 OpenRC 缺失。
3. **需要正常启动 shell**：如果要改 inittab，建议先明确备份和恢复方式，并注意 `stty -echo` 的持久影响。
4. **板卡分配**：如果一直被分到不需要的镜像，联系平台管理员固定租约或统一两块板的镜像；`--board-id` 目前不可靠。
5. **脚本自动化**：不要假设 `ostool board connect` 一定连到某块板；从日志里解析 `board_id` / `session_id`，并对 Alpine/Yocto 分别处理。

> 把 Alpine 修成可正常使用的 Linux 的完整方案对比与执行步骤，见 [`05-os-options.md`](05-os-options.md)。
