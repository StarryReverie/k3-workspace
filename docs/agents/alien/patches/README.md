# Alien 补丁

基准提交：`181b263b`

应用顺序：

1. `0001-platform-detect-plic-node.patch`
2. `0002-kernel-skip-missing-kallsyms.patch`
3. `0003-k3-minimal-platform.patch`

应用方式：

```bash
cd /path/to/Alien
for patch in /path/to/k3/docs/agents/alien/patches/000*.patch; do
    git apply --check "$patch"
    git apply "$patch"
done
```

三个补丁都用于本地调试，尚未提交到上游。

- `0001`、`0002` 属于 QEMU 基线修复。
- `0003` 属于 K3 最小平台，包含 `k3` feature、平台模块、SBI 标准扩展、initramfs 解析、SBI 控制台、链接脚本与 FIT。
