# Windows 0.0.6 验收记录

当前验收候选：`0.0.6-win-x64-02`，源码提交 `3143c49dfbec91c37b1110722cdacc6104c22885`，Windows x64 / MSVC。记录日期：2026-10-02。状态：`IN_PROGRESS`。

本表只登记本次候选证据。历史 0.0.5 报告保留为来源说明；后续源码变化与最终 main 合并构建使用新的不可变候选和 Release 清单。

## 自动检查

证据根目录为 `work/evidence/`；本机完整路径为 `D:\Codex\Atrio-WorkSpace-Windows-006\work\evidence`。

| 检查 | 状态 | 本次证据 |
|---|---|---|
| npm 锁定依赖安装 | `PASS` | `candidate-02-npm-ci.log`，219 packages，0 vulnerabilities |
| npm/Cargo/Tauri 与渠道版本 | `PASS` | `candidate-02-version.log`，0.0.6，Dev/Beta 身份分离 |
| 共享前端测试 | `PASS` | `candidate-02-frontend-final.log`、`candidate-02-frontend-tests-scoped.log`，8 文件 / 82 项 |
| Dev/Beta 前端生产构建 | `PASS` | `candidate-02-frontend-dev.log`、`candidate-02-frontend-beta.log` |
| 候选/runtime fixture | `PASS` | `candidate-02-runtime-candidate-fixtures.log`，14 passed / 3 Darwin 专属 skips / 0 failed |
| Windows Rust 测试 | `PASS` | `candidate-02-rust-tests.log`，120 passed / 0 failed / 3 expected ignored；真实模型测试未执行 |
| Windows Rust 编译检查 | `PASS` | `candidate-02-cargo-check.log` |
| diff 检查 | `PASS` | `candidate-02-diff-check.log`，无输出错误 |
| GitHub CI 与 macOS 回归 | `PASS` | [运行 37004579179](https://github.com/Jaime-Gu/atrio-agent-workspace/actions/runs/37004579179) 四项 SUCCESS；`candidate-02-ci.log`：两个平台前端 82 项，Windows MSVC 120 项，macOS arm64 103 项，原生编译均通过 |

`candidate-02-frontend-tests.log` 保留了首次误扫描 `work/` 导入依赖测试的失败诊断；前端通过结果以明确限定 `src/` 的 scoped/final 日志为准。macOS CI 使用固定来源准备临时 runtime，只提供编译与 fixture 回归证据，不替代 macOS 发布包验收。

## 原生与安装

| 检查 | 状态 | 本次证据与范围 |
|---|---|---|
| Dev/Beta 身份及数据目录分离 | `PASS` | `native-02/dev-app-info.txt`、`beta-app-info.txt`：0.0.6、候选 02、同一源码指纹；标识分别为 `dev.pixel.workspace.dev` / `dev.pixel.workspace`，Roaming 数据目录不同 |
| 打包前端独立启动 | `PASS` | `native-02/installed-dev-restored.txt`、`installed-beta-startup.txt`；生产窗口使用 `tauri.localhost` |
| Mock 提案、审批与计划/看板 | `PASS` | `native-02/plan-approved.*`、`board-plan-25.txt`：计划批准，Planner 与 Dashboard 同步为 1/4、25% |
| 文档 revision、拒绝与批准写入 | `PASS` | `native-02/revision-conflict.txt`、`rejected-draft-preserved.txt`、`document-before-write.txt`；外部修改导致旧审批冲突且文件哈希保持，拒绝保留草稿，新的批准原子写入成功 |
| 权限撤销/恢复询问 | `PASS` | `native-02/permission-revoked-confirmed.txt` 确认系统策略“禁止运行”，随后恢复“询问” |
| 关闭/重启恢复 | `PASS` | `native-02/installed-dev-restored.txt`、对应 PNG；4 模块、已保存文档和 Planner/Dashboard 25% 恢复 |
| 原生停止与进程回收 | `PASS`（synthetic ACP） | `native-02/acp-cancelled.txt`、`acp-before-cancel-processes.json`、`acp-after-cancel-processes.json`：持续任务取消，应用创建的 parent/child 回收为 0。Mock 停止尝试时已 completed，`stopped-run*.txt` 不计通过 |
| ACP 协议与两轮同会话 | `PASS`（synthetic ACP） | `native-02/acp-completed-turn-{1,2}.txt`、`acp-session-reuse-verified.json`：两轮完成且复用同 session，真实模型调用 0 |
| GUI 工作区切换 | `PASS` | `native-02/workspace-switched-confirmed.txt`、`acp-after-switch-confirmed.json`：运行中切换回收 parent/child 为 0，工作区 B 的 3 模块持久化 |
| 本机随包 Codex ACP initialize | `PASS`（协议握手） | `work/evidence.noindex/codex-initialize-only-verified.json`：实际 ACP 1.13.1 / Node 22.23.3、协议 1，312ms；私有 Job、leader、读取/写入线程均回收，未发 newSession/prompt/模型或文件请求 |
| Dev/Beta NSIS 安装及资源完整性 | `PASS` | `candidate-02-dev-install.json`、`beta-install.json`：exit 0；安装目录含空格与中文。安装 runtime 精确 28 文件 / 88,685,465 bytes，见 `candidate-02-beta-installed-runtime.json` |
| Beta 重新安装 | `PASS` | `candidate-02-beta-reinstall.json`、`beta-reinstalled-runtime.json`：exit 0，工作区数据库保留，runtime 哈希一致 |
| 移动完整运行目录 | `PASS` | `candidate-02-relocated-runtime.json`、`native-02/relocated-codex-launch-plan.txt`：完整 Beta 目录移走后旧目录不存在，运行 EXE 路径为新目录，原生探测定位相邻 `agents/codex/bin/node.exe` 与 ACP 1.13.1 |
| 安装后生产 EXE MCP stdio | `PASS`（synthetic Host） | `candidate-02-installed-dev-mcp.json`、`installed-beta-mcp.json`，各 11 checks；`providerCalls=0`、`workspaceDatabasesOpened=0` |
| AppContainer 修复与旧工作区副本打开 | `PASS` | Windows 路径/containment 回归通过；`native-02/legacy-copy-open-summary.json`：完整备份副本原生打开，3 模块、无加载失败、未删除数据库 |
| 缺 adapter/CLI 明确错误 | `PASS`（GUI） | `native-02/missing-adapter-error.txt`、`missing-cli-confirmed.txt` / PNG：入口缺失要求完整安装，缺 Codex 宿主显示明确 CLI 路径原因并展开诊断，session/auth 为 not_checked，无 Mock fallback；隔离测试进程环境，模型与认证请求 0 |
| 未登录后的真实认证/会话失败 | `NOT_TESTED_BY_USER_SCOPE` | 本机官方 CLI `0.159.0-alpha.12.1` 为 `NotLoggedIn`，UI 提供官方登录说明；未执行真实 newSession 或登录 |

安装、fixture、握手、原生界面与真实模型调用分别登记，不能互相代替。旧工作区仅使用完整备份或隔离副本，不通过删除 SQLite 排除问题。

候选 02 验收后补充源码：连接汇总现在以 Host 当前状态显示“未连接/连接中/停止中/已连接”，历史 probe 标为“上次检查结果”，不伪报连接；新增 6 项共享前端回归（总计 88 项）、Windows 四来源字段与旧 manifest 兼容回归（候选 fixture 总计 15 passed / 3 Darwin skips）、synthetic ACP 与实际 Codex initialize-only harness。该补充源码及最终 main 提交必须重新冻结构建，不能改写候选 02 身份。

本机 CLI 来源分别记录：原生 GUI 版本探测发现 `pi-node/current/codex.cmd` 的 `codex-cli 0.160.0`；initialize-only 明确使用官方 Desktop CLI `0.159.0-alpha.12.1`。两者均为外部用户安装，随包固定版本仅 ACP 1.13.1 / Node 22.23.3。

## Provider 范围与发布状态

- Hermes：`SKIPPED_BY_USER`，本机未安装。
- Claude：`NOT_TESTED_BY_USER_SCOPE`，用户暂不登录。
- Codex 真实认证/模型调用：`NOT_TESTED_BY_USER_SCOPE`，继续用户后置验收；没有复制认证或发送真实模型任务。

[PR #2](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/2) 当前为面向 `windows/integration` 的整合草稿。最终 main 提交、发布候选、安装包 SHA-256 与 Release 地址由最终不可变 Release 清单记录；当前候选 02 的身份和历史证据保持不变。
