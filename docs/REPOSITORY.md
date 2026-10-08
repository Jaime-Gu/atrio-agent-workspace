# 目录与分支

0.0.7[1] 多对话协作使用 [编码分区规划](WORKTREE_PARTITIONS.md)。对应角色分支以 `0.0.6` 标签和共同的分区准备提交开始；各平台运行时准备与验收仍遵守平台说明。

## 当前来源

共享版本为 `0.0.6`。本次 Windows 从 `0.0.5-win-x64-codex-10` 逐文件导入，历史来源 `0.0.5-atrio-02` 与原始 `darwin/arm64` provenance 保留。接收基线为 main `401ad96175561a097eb485521bc1416d4f5a2f1c`、windows/integration `793c12551349452a8ff3a1a18cefbf56640fc3d0`。

Windows 发布候选 `0.0.6-win-x64-03` 来自 main 发布源码提交 `53e45575bfd6f0d8de7f1c1e5ada64862dfc859c`，源码指纹 `sha256:e90729d776580b34f50a53e4da04b118f0c2814d0c862b6eecfb128f01efbee3`。[#2](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/2) 已整合到 `windows/integration`，[#3](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/3) 已合并 main。历史 `0.0.5-atrio-12` 标签继续指向 `7b6fc1817bb5c9afabbc80f3c674effd78a5d443`。

## 目录职责

| 路径 | 内容 |
|---|---|
| `src/` | 共享 React 界面、类型与交互 |
| `src-tauri/src/` | Rust Host、模块工具、权限与持久化 |
| `src-tauri/src/platform/` | 实际 Windows 进程、IPC、锁、路径、文件操作和对应 macOS 实现 |
| `src-tauri/tauri.conf.json` | Beta 基础配置，保留 macOS 资源与打包设置 |
| `src-tauri/tauri.dev.conf.json` | Dev 渠道配置 |
| `src-tauri/tauri.windows.conf.json` | Windows NSIS、ICO、WebView2 与 runtime 覆盖 |
| `scripts/` | 共享版本、候选及现有 macOS 入口 |
| `scripts/windows/` | Windows runtime 准备/验证、Dev/Beta 构建、安装与 MCP fixture |
| `scripts/runtime/` | Windows x64 runtime lock 与来源说明 |
| `docs/platforms/` | 实际平台状态、构建方法与限制 |
| `.github/workflows/` | 两个平台共享前端、Windows MSVC 与 macOS arm64 原生回归 |
| `work/` | 被忽略的依赖、运行时二进制、构建与验收记录 |

npm manifest/lock 位于仓库根目录；Cargo manifest/lock 位于 `src-tauri/`。平台接口变化同步更新调用方和测试，macOS 现有 runtime 脚本与 lock 保留。共享 Host 统一处理 DTO、审批、revision、权限与数据库，平台实现不降低保护或回退 Mock。

## 验收与发布

最终候选 03 的自动检查、Dev/Beta 完整 NSIS 安装与关键原生复验见 [Windows 验收记录](ACCEPTANCE-0.0.6-WINDOWS.md)。[CI 37032940358](https://github.com/Jaime-Gu/atrio-agent-workspace/actions/runs/37032940358) 在发布源码提交上四项 SUCCESS：前端 88 项、Windows MSVC 120 项与 macOS arm64 103 项原生测试及编译。macOS 首次目录锁时序失败后在同一提交重跑通过，首次失败日志保留。临时 macOS runtime 仅用于编译/fixture 回归，不是 Mac 安装包验收。

最终 03 复验与早期 02 的全面原生覆盖分开登记：03 覆盖安装与资源、生产 MCP stdio、打包前端、渠道/内嵌身份、恢复、取消/退出进程回收、当前 session 摘要与隔离旧工作区副本。02 的审批、revision、权限、ACP 两轮、切换、重装、移动目录和错误诊断是沿用证据，不冒充 03 的逐项重跑。Hermes `SKIPPED_BY_USER`，Claude/Codex 真实认证与模型调用 `NOT_TESTED`。

成果已按任务分支 → `windows/integration` → main 的 PR 流程合并，未改写历史。发布源码、候选 03 与安装包身份固定，发布后文档更新只登记事实。[Windows 0.0.6 Release](https://github.com/Jaime-Gu/atrio-agent-workspace/releases/tag/0.0.6) 已发布，标记为 Pre-release；发布时间 2026-10-03 01:43:19（Asia/Shanghai）。仓库为 PRIVATE，下载 Release 附件需要具备该仓库的 GitHub 访问权限。

发布后已通过有仓库权限的 GitHub 会话重新下载全部 12 个附件，逐文件 SHA-256 与已验收 staging 产物一致；`SHA256SUMS` 的 11 个条目及清单自身哈希均通过核对。两份安装器 SHA-256 保持上述固定值。独立复核证据为 `work/evidence/release-download-verification.json` 与 `release-download-verified-root.json`；未声明匿名下载可用。 附件按 `Atrio-WorkSpace-<版本>-<渠道>-<平台>-<架构>.<扩展名>` 命名，Dev/Beta NSIS 字节数与 SHA-256 见 [来源记录](PROVENANCE-0.0.6.md)。

分支与独立工作树规则见 [CONTRIBUTING.md](../CONTRIBUTING.md)；详细来源见 [来源记录](PROVENANCE-0.0.6.md)。
