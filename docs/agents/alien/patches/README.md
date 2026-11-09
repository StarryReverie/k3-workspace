# Alien 补丁

基准提交：`181b263b`

应用顺序：

1. `0001-platform-detect-plic-node.patch`
2. `0002-kernel-skip-missing-kallsyms.patch`

应用方式：

```bash
cd /path/to/Alien
git apply --check /path/to/k3/docs/agents/alien/patches/0001-platform-detect-plic-node.patch
git apply /path/to/k3/docs/agents/alien/patches/0001-platform-detect-plic-node.patch
```

两处修改都用于本地调试，尚未提交到上游。K3 平台补丁将在阶段 1 单独增加，避免与 QEMU 基线的修复混在一起。
