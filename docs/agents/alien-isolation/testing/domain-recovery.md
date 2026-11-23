# 域崩溃恢复与热替换

适用硬件：QEMU `virt`，riscv64，2 个 hart
适用系统：AlienOS `isolation`
适用工具：`nightly-2024-10-25`、QEMU 9 及以上
记录时间：2026-11-23
相关文档：[`qemu-matrix.md`](qemu-matrix.md)、
[`../plan/roadmap.md`](../plan/roadmap.md)

本文记录 M6 的隔离性验证：一个块设备域在运行中崩溃后，TCB 与文件系统
是否仍然可用；以及把一个正在使用的块设备域热替换为新版本后，已挂载的
FAT 是否还能读。两者都不需要 K3 硬件。

## 1. 故障注入机制

Alien 的驱动域默认带 `rust-unwind`。域内 panic 由 `basic::catch_unwind`
捕获，转换成 `AlienError::DOMAINCRASH` 返回给调用者，而不是让整个系统退出。

块设备域另有一次性注入点：

```rust
// domains/drivers/virtio_mmio_block/virtio_mmio_block/src/lib.rs
#[cfg(feature = "crash")]
if basic::blk_crash_trick() {
    panic!("blk crash trick");
}
```

TCB 侧：

```rust
// kernel/src/domain_helper/syscall.rs
static BLK_CRASH: AtomicBool = AtomicBool::new(true);

fn sys_backtrace(&self, domain_id: u64) {
    // 累加该域的 panic_count
    unwind();   // BLK_CRASH = false
}
```

因此注入只会发生一次：第一次读到块设备时域 panic，TCB 记录并关闭注入，
`shadow_blk` 捕获 `DOMAINCRASH` 后重读，第二次读取成功。

## 2. 构建

普通镜像不启用注入。测试镜像单独构建：

```bash
./scripts/alien-isolation/build-qemu.sh \
    --smp 2 --log WARN \
    --out alien-isolation/build/crash-smp-2 \
    --crash
```

`--crash` 通过 `cargo domain build-all --features virtio_mmio_block=crash`
只对块设备域启用 feature，其余域不受影响。`build-info` 会记录 `CRASH=1`，
测试脚本据此区分普通镜像与注入镜像，二者不会互相覆盖。

`crash` feature 需要在 `gvirtio_mmio_block` 上透传到实现 crate：

```toml
crash = ["virtio_mmio_block/crash"]
```

## 3. 场景

测试入口为 `scripts/alien-isolation/test-qemu.py`。

| 场景 | 核数 | 验证内容 |
|---|---|---|
| `blk-crash` | 2 | 注入 panic、`catch unwind`、`shadow_blk` 重读、启动到 shell、文件内容正确 |
| `block-swap` | 2 | 不支持的替换被拒绝；`shadow_blk-1` 换成 `sshadow_blk`；FAT 仍可读 |

```bash
./scripts/alien-isolation/test-qemu.py --build blk-crash block-swap
```

`all` 已包含这两个场景。

### 3.1 `blk-crash`

启动时 task 域从 `/tests/init` 读取块设备，第一次读触发注入。串口应出现：

```text
[0][Domain:N] PanicInfo { message: blk crash trick, location: ... }
[0][Domain:N] catch unwind error
[0][Domain:N] [ERROR] ["shadow_blk/src/lib.rs":74] domain crash, try restart domain
[0][Domain:N] try reread block
```

随后系统继续挂载 FAT 并进入 shell。场景还要求：

```text
M6_FILE:alien isolation block read ok
M6_CRASH_OK
```

前者证明重试后读到正确内容，后者证明 TCB 仍然存活。场景把
`panicked at` / `PanicInfo` 视为预期，但 `Kernel panic` 仍是失败条件。

### 3.2 `block-swap`

`dtest` 先发起一个 TCB 无法执行的替换（把 `ramfs-1` 换成块域），要求
返回错误而不是 panic：

```text
[1] <sys_update_domain> replace Some(FsDomain) not supported
SWAP_REJECT_OK
```

然后正常替换正在使用的块域，并读取文件：

```text
[0] Try to replace domain: shadow_blk-1 with domain: sshadow_blk ok
SWAP_OK
M6_SWAP_OK
```

`shadow_blk-1` 带后缀是因为内核用非唯一方式注册，实际名称由注册计数生成。

## 4. TCB 加固

`sys_update_domain` 与 `sys_reload_domain` 原本在遇到不支持的域类型时
直接 `panic!`。这两个入口可由普通域发起，等同于允许一个域打崩可信基。
现在改为打印一行诊断并返回 `AlienError::EINVAL`，由 `block-swap` 覆盖。

## 5. 边界

- 注入只覆盖 QEMU 的 `virtio_mmio_block`。K3 的 `k3_sd` 与 `nvme` 没有
  等价的注入点，板端未做崩溃恢复测试。
- 注入是一次性的，只在首次读块时触发；重启后 TCB 的 `BLK_CRASH` 重新为真。
- 热替换只支持 `sys_update_domain` 已实现的域类型；其余类型返回 `EINVAL`。
- `sys_reload_domain` 目前仍没有用户态调用者，只验证了拒绝路径不 panic。
