# 平台运行时清单

当前 macOS Codex 运行时锁位于 `scripts/codex-runtime.lock.json`，由 `scripts/package-codex-runtime.mjs` 读取。现有路径继续保留。

平台清单在本目录按照 `codex-runtime.lock.<平台>-<架构>.json` 命名。清单迁移时，同时更新准备脚本、验证脚本与候选绑定代码，明确每个平台读取的文件。

清单需要记录资源来源、Node 与 ACP 版本、目标平台与架构、入口、每个文件的 SHA-256、字节数以及许可证。资源二进制准备到 `work/resources.noindex/`，按平台和架构使用独立目录。

Windows 清单使用本机目标文件生成。macOS 文件权限和 Mach-O 签名信息按照 macOS 规则记录；Windows 根据 PE 文件和实际启动入口验证。
