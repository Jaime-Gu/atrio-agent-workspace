# Windows 构建脚本目录

状态：`PENDING_SOURCE_IMPORT`。本目录接收 Windows 原生构建、Codex 资源准备、候选冻结和安装包校验脚本。

脚本输入应明确源码、渠道、架构和运行时清单；依赖缺失、版本冲突或哈希变化时直接报错。打包之前核实 Node、ACP、图标、WebView2 和目标平台文件。

输出写入被 Git 忽略的 `work/`。源码候选、运行时和安装包分别记录 SHA-256；构建记录包含提交 SHA 与 Windows 实际架构。

平台接入和验收见 [Windows 文档](../../docs/platforms/windows.md)。
