# Windows 0.0.6

Windows 已按仓库平台目录接入本机 `0.0.5-win-x64-codex-10` 成果，保留共享 0.0.6 界面、DTO、审批、revision、权限、工作区切换、持久化与恢复语义，以及 Windows Ctrl 快捷键提示。目标为 Windows x64 / `x86_64-pc-windows-msvc`。

## 来源与目录

历史来源为 `0.0.5-atrio-02`，本机导入候选为 `0.0.5-win-x64-codex-10`。原始 `darwin/arm64` provenance 保留不变。本次预发布验收候选为 `0.0.6-win-x64-02`，提交 `3143c49dfbec91c37b1110722cdacc6104c22885`；来源见 [来源记录](../PROVENANCE-0.0.6.md)。接收分支为 `windows/integration`，最终 main 合并构建另冻结新候选。

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

## 当前验收状态

候选 02 的 npm/version、前端 82 项、Dev/Beta 前端构建、runtime/candidate fixture、Windows Rust 120 项与编译检查均通过。GitHub [CI 37004579179](https://github.com/Jaime-Gu/atrio-agent-workspace/actions/runs/37004579179) 四项 SUCCESS，包含 macOS arm64 原生编译/103 项测试和 Windows MSVC 回归。macOS 临时 runtime 仅用于编译回归，不是发布包证据。

Dev/Beta NSIS 已安装到含空格和中文的隔离目录；28 文件 / 88,685,465 bytes runtime 完整。Beta 重装保留数据库。安装后生产 EXE MCP stdio 各 11 项通过，真实 provider/database 调用均为 0。原生验收已覆盖 Dev/Beta 身份和数据目录、Mock 审批、revision 冲突、拒绝草稿保留、批准写入、权限禁止/恢复询问、计划/看板 25% 更新及重启恢复。

Synthetic ACP 已在原生 GUI 完成同 session 两轮、持续任务取消及切换时的进程树回收；工作区 B 的 3 模块持久化，旧工作区完整备份副本成功打开。完整 Beta 运行目录真正移走后，原生 EXE 与 launch plan 从新位置定位 runtime。缺 adapter/CLI 的错误和自动展开诊断已验证。实际随包 Codex ACP initialize-only 握手通过且私有 Job/线程回收；未发送真实 newSession/prompt。Mock 停止尝试时任务已完成，未记通过。完整分项与日志路径见 [Windows 验收记录](../ACCEPTANCE-0.0.6-WINDOWS.md)。

补充源码修正当前会话与历史探测状态的区分，新增 6 项共享前端回归（总计 88 项）；候选来源拆为四个根字段并嵌入完整历史 provenance，兼容旧 manifest。该补充与最终 main 提交另冻结新候选，历史候选 02 身份不变。

- Hermes：`SKIPPED_BY_USER`，本机未安装。
- Claude：`NOT_TESTED_BY_USER_SCOPE`，用户暂不登录。
- Codex：真实认证/模型调用 `NOT_TESTED_BY_USER_SCOPE`；本机 CLI `0.159.0-alpha.12.1` 为 `NotLoggedIn`，真实验收继续用户后置。

当前为整合预发布验收，最终 main 提交重新构建与发布附件以不可变 Release 清单为准。安装包尚未建立签名/SmartScreen 声誉，发布说明须保留未测项目和路径限制。源码仓库不提交运行时二进制、target、node_modules、认证、用户工作区或未脱敏日志。
