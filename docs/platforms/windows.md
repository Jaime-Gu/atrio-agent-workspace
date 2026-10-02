# Windows 0.0.6

Windows 已按仓库平台目录接入本机 `0.0.5-win-x64-codex-10` 成果，保留共享 0.0.6 界面、DTO、审批、revision、权限、工作区切换、持久化与恢复语义，以及 Windows Ctrl 快捷键提示。目标为 Windows x64 / `x86_64-pc-windows-msvc`。

## 实际验证系统与安装要求

实机验收系统为 Windows 11 Home 中文版，`10.0.26200` / build `26200`，x64。当前安装包和 Rust 目标仅为 x64；Windows ARM64 不属于本次目标，其他 Windows 版本/版本组合尚未实机验收。NSIS 使用 `currentUser` 安装；若系统没有 WebView2，`downloadBootstrapper`（silent）安装方式需要网络下载 WebView2。

UNC、网络映射盘和工作区内部 junction/符号链接等 reparse 路径限制继续保留。

## 来源与目录

历史来源为 `0.0.5-atrio-02`，本机导入候选为 `0.0.5-win-x64-codex-10`。原始 `darwin/arm64` provenance 保留不变。最终发布候选为 `0.0.6-win-x64-03`，从合并后的 main `53e45575bfd6f0d8de7f1c1e5ada64862dfc859c` 冻结，工作树 clean；源码指纹 `sha256:e90729d776580b34f50a53e4da04b118f0c2814d0c862b6eecfb128f01efbee3`。[#2](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/2) 已合并 `windows/integration`，[#3](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/3) 已合并 main。来源见 [来源记录](../PROVENANCE-0.0.6.md)。

| 目录 | 实际职责 |
|---|---|
| `src/` | 两个平台共享界面、类型和交互 |
| `src-tauri/src/platform/windows/` | Windows 进程/Job Object、环境、IPC、锁、物理路径与文件操作 |
| `src-tauri/src/platform/macos/` | 对应 macOS 实现，保留条件编译与行为 |
| `src-tauri/src/platform/mod.rs` | 公共接口与平台分派 |
| `scripts/windows/` | runtime 准备/验证、Dev/Beta 构建、安装和 MCP fixture |
| `scripts/runtime/` | Windows x64 实际文件生成的 runtime lock |
| `src-tauri/tauri.windows.conf.json` | Windows ICO、NSIS、WebView2 与资源映射，和渠道配置组合 |
| `work/` | 被忽略的依赖、二进制、构建及验收证据 |

Windows 启动采用 `CREATE_SUSPENDED` → 绑定 Job → 恢复；取消、断开、退出和工作区切换回收应用创建的进程树与读取线程。独立启动的用户 Codex 不归该 Job 管理。Named Pipe 使用当前用户 ACL，并保留 session/run scope、token、长度和超时检查。

## 构建

```powershell
npm ci
npm run version:check
npm test
npm run build:dev
npm run build:beta
npm run windows:runtime:prepare
npm run windows:runtime:verify
npm run windows:dev:build
npm run windows:beta:build
```

准备流程使用仓库内 adapter manifest/lock，在 `work/` 执行锁定安装，并按官方来源和 SHA-256 下载 Windows Node。固定 ACP `1.13.1`、Node `22.23.3`。payload 包含 `bin/node.exe`、`adapter/index.js`、最小 package manifest、`manifest.json` 和许可证，启动计划为 `node.exe adapter/index.js`。官方 Codex CLI、登录和个人配置由本机用户管理。

NSIS runtime 位于 EXE 相邻 `agents/codex`；resolver 同时兼容 Dev `resources/agents/codex` 布局，保留 candidate-10 安装修复。AppContainer `LocalCache\Roaming` alias、临时文件物理父目录及 SQLite containment 修复保留。UNC、网络映射盘和不受信任 reparse 路径仍不支持，不放宽目录边界检查。

## 最终候选 03 验收与安装

最终 main 的 npm/version、共享前端 88 项、Dev/Beta 前端和完整 NSIS 构建、15 项 runtime/candidate fixture（3 Darwin 专属 skips）、Windows Rust 120 项及编译检查均通过。[CI 37032940358](https://github.com/Jaime-Gu/atrio-agent-workspace/actions/runs/37032940358) 四项 SUCCESS，覆盖两平台前端、Windows MSVC 与 macOS arm64 原生编译/103 项测试。macOS 首次运行有目录锁 150ms 重试窗口的时序失败（102 passed / 1 failed）；同一源码重跑 103 passed / 0 failed，未改断言或跳过测试。

候选 03 的 Dev/Beta NSIS 已安装到含空格和中文的隔离目录，安装 exit 0，随包资源均核实为 28 文件 / 88,685,465 bytes。生产 EXE MCP stdio 各 11 项通过（Provider 与数据库调用均为 0）。本次最终原生复验覆盖：打包前端启动、两渠道 app_info/内嵌身份、Dev 已保存模块与 Planner/Dashboard 25% 恢复、synthetic ACP 取消与正常退出的私有进程树回收、取消后当前 session 汇总“未连接”、Beta 隔离旧工作区备份副本打开及原用户 policy/selection 恢复。证据为 `work/evidence/native-final-03/summary.json`、`final-03-*` 日志。

候选 02 的全面原生验收仍按 02 单独列出，包括 Mock 批准/拒绝、revision 冲突、权限撤销、ACP 两轮、工作区切换、重装、移动完整运行目录和缺 adapter/CLI 错误；这些是沿用的实现回归证据，不声称在最终 03 安装包上逐项重跑。实际随包 Codex initialize-only 握手也是单独协议证据，不等同真实模型。详细范围见 [Windows 验收记录](../ACCEPTANCE-0.0.6-WINDOWS.md)。

| 完整 NSIS 安装包 | 字节数 | SHA-256 |
|---|---:|---|
| [Atrio-WorkSpace-0.0.6-dev-windows-x64.exe](https://github.com/Jaime-Gu/atrio-agent-workspace/releases/download/0.0.6/Atrio-WorkSpace-0.0.6-dev-windows-x64.exe) | 25,980,376 | `77ca4c3f0678ac39c936553ba26d909967b139b703a8c809df86d4a5aa251225` |
| [Atrio-WorkSpace-0.0.6-beta-windows-x64.exe](https://github.com/Jaime-Gu/atrio-agent-workspace/releases/download/0.0.6/Atrio-WorkSpace-0.0.6-beta-windows-x64.exe) | 25,985,970 | `045c4201506bf4f957f938f88074df94e8272b93f82185f8329b9a1c56d401b7` |

[Windows 0.0.6 Release](https://github.com/Jaime-Gu/atrio-agent-workspace/releases/tag/0.0.6) 已发布，标记为 Pre-release；发布时间 2026-10-03 01:43:19（Asia/Shanghai）。仓库为 PRIVATE，下载 Release 附件需要具备该仓库的 GitHub 访问权限。

发布后已通过有仓库权限的 GitHub 会话重新下载全部 12 个附件，逐文件 SHA-256 与已验收 staging 产物一致；`SHA256SUMS` 的 11 个条目及清单自身哈希均通过核对。两份安装器 SHA-256 保持上述固定值。独立复核证据为 `work/evidence/release-download-verification.json` 与 `release-download-verified-root.json`；未声明匿名下载可用。

关闭正在运行的同渠道应用并备份工作区后，运行对应 NSIS；Dev 使用 `dev.pixel.workspace.dev`，Beta 使用 `dev.pixel.workspace`，数据目录分离。升级保留工作区与本机权限设置，官方 CLI 与认证由用户自行维护。安装包包含完整 runtime，无需开发服务器；移动运行目录时应移动完整目录。

## 早期候选 02 的完整回归记录

候选 02 的 npm/version、前端 82 项、Dev/Beta 前端构建、runtime/candidate fixture、Windows Rust 120 项与编译检查均通过。GitHub [CI 37004579179](https://github.com/Jaime-Gu/atrio-agent-workspace/actions/runs/37004579179) 四项 SUCCESS，包含 macOS arm64 原生编译/103 项测试和 Windows MSVC 回归。macOS 临时 runtime 仅用于编译回归，不是发布包证据。

Dev/Beta NSIS 已安装到含空格和中文的隔离目录；28 文件 / 88,685,465 bytes runtime 完整。Beta 重装保留数据库。安装后生产 EXE MCP stdio 各 11 项通过，真实 provider/database 调用均为 0。原生验收已覆盖 Dev/Beta 身份和数据目录、Mock 审批、revision 冲突、拒绝草稿保留、批准写入、权限禁止/恢复询问、计划/看板 25% 更新及重启恢复。

Synthetic ACP 已在原生 GUI 完成同 session 两轮、持续任务取消及切换时的进程树回收；工作区 B 的 3 模块持久化，旧工作区完整备份副本成功打开。完整 Beta 运行目录真正移走后，原生 EXE 与 launch plan 从新位置定位 runtime。缺 adapter/CLI 的错误和自动展开诊断已验证。实际随包 Codex ACP initialize-only 握手通过且私有 Job/线程回收；未发送真实 newSession/prompt。Mock 停止尝试时任务已完成，未记通过。完整分项与日志路径见 [Windows 验收记录](../ACCEPTANCE-0.0.6-WINDOWS.md)。

候选 02 后的共享会话摘要与来源字段修正已包含于最终提交和候选 03；历史候选 02 身份不变。

- Hermes：`SKIPPED_BY_USER`，本机未安装。
- Claude：`NOT_TESTED`，用户暂不登录。
- Codex：真实认证/模型调用 `NOT_TESTED`；本机 CLI `0.159.0-alpha.12.1` 为 `NotLoggedIn`，真实验收继续用户后置。

最终安装包固定对应发布源码提交和候选 03；发布后文档状态更新不会重建安装包、改写候选或移动版本 tag。两份最终 NSIS 的 Authenticode 状态均为 `NotSigned`，SmartScreen 声誉未建立，发布说明须保留未测项目和路径限制。源码仓库不提交运行时二进制、target、node_modules、认证、用户工作区或未脱敏日志。
