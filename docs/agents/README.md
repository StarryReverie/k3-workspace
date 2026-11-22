# Agent 工作记录

本目录保存排查过程、实测数据与阶段性方案。面向读者的正式文档位于上一级 `docs/`。

## 目录结构

| 路径 | 内容 |
|---|---|
| `01-platform.md` 至 `05-os-options.md` | K3 平台模型、U-Boot、Alpine 修复与方案对比 |
| `alien/` | AlienOS 宏内核版本的调研、QEMU 基线、K3 移植记录 |
| `alien-isolation/` | AlienOS `isolation` 分支的源码导入与隔离域移植记录 |
| `doc-conventions.md` | 文档与用语规范 |

仓库根目录的 `alien/` 与 `alien-isolation/` 是 Alien 源码的 vendored 快照，
不是工作记录。旧树的来源见 `alien/VENDORED.md`；隔离树的来源、工具链与
更新方式见 `docs/agents/alien-isolation/setup/vendoring.md`。

## 新增主题的约定

- 一个主题一个子目录，入口使用 `README.md`；主题内按阶段或模块再分目录。
- 新增文档不使用数字前缀。既有数字前缀文件保持原样，不单独重命名。
- 本地改动直接提交，不单独维护补丁目录；日志节选放入 `logs/`。
- 关键版本、命令、验收结果与未验证项必须写入记录。
- 不写口令、令牌、私钥与会话 cookie。
- 临时试验产物不提交；需要复现时由脚本重新生成。
