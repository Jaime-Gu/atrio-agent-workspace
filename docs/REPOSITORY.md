# 目录与分支

0.0.7[1] 多对话协作使用 [编码分区规划](WORKTREE_PARTITIONS.md)。对应角色分支以 `0.0.6` 标签和共同的分区准备提交开始；各平台运行时准备与验收仍遵守平台说明。

## 当前来源

共享版本为 `0.0.6`。本次 Windows 从 `0.0.5-win-x64-codex-10` 逐文件导入，历史来源 `0.0.5-atrio-02` 与原始 `darwin/arm64` provenance 保留。接收基线为 main `401ad96175561a097eb485521bc1416d4f5a2f1c`、windows/integration `793c12551349452a8ff3a1a18cefbf56640fc3d0`。

当前预发布验收候选 `0.0.6-win-x64-02` 来自提交 `3143c49dfbec91c37b1110722cdacc6104c22885`。[PR #2](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/2) 为整合草稿，base 为 `windows/integration`。历史 `0.0.5-atrio-12` 标签继续指向 `7b6fc1817bb5c9afabbc80f3c674effd78a5d443`。

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

候选 02 的自动检查、Dev/Beta NSIS 安装和当前原生验收见 [Windows 验收记录](ACCEPTANCE-0.0.6-WINDOWS.md)。[CI 37004579179](https://github.com/Jaime-Gu/atrio-agent-workspace/actions/runs/37004579179) 四项全部 SUCCESS：macOS/Windows 前端各 82 项，Windows MSVC 120 项与 macOS arm64 103 项原生测试及编译通过。macOS CI 的临时 runtime 只作为编译回归，不作为发布包验收。

安装、原生 UI、协议 fixture 与真实模型调用分项记录。Synthetic ACP 原生两轮、取消/切换回收、旧工作区备份副本、移动运行目录及缺 adapter/CLI 明确错误均已验证；实际 Codex 只执行 initialize-only 握手，未执行真实 session/prompt。Hermes 跳过，Claude/Codex 真实认证和模型调用为 `NOT_TESTED_BY_USER_SCOPE`。历史报告不能当作新候选通过证据。

成果先整合到 `windows/integration`，再通过 PR 合并 main，遵守分支保护与审批。最终 main 提交重新冻结和构建，最终不可变 Release manifest 记录提交、候选、源码/runtime/安装器哈希与未测项目，不预填未知 Release。附件按 `Atrio-WorkSpace-<版本>-<渠道>-<平台>-<架构>.<扩展名>` 命名；不移动旧 tag、不覆盖 Mac 附件，发布后下载核对 SHA-256。

分支与独立工作树规则见 [CONTRIBUTING.md](../CONTRIBUTING.md)；详细来源见 [来源记录](PROVENANCE-0.0.6.md)。
