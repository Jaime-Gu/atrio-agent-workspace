# Windows 0.0.6 验收记录

最终发布候选：`0.0.6-win-x64-03`，发布源码提交 `53e45575bfd6f0d8de7f1c1e5ada64862dfc859c`，Windows x64 / `x86_64-pc-windows-msvc`。记录日期：2026-10-03（Asia/Shanghai）。源码指纹：`sha256:e90729d776580b34f50a53e4da04b118f0c2814d0c862b6eecfb128f01efbee3`。最终 main 冻结时工作树 clean；[#2](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/2) 和 [#3](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/3) 已合并。

[Windows 0.0.6 Release](https://github.com/Jaime-Gu/atrio-agent-workspace/releases/tag/0.0.6) 已发布，标记为 Pre-release；发布时间 2026-10-03 01:43:19（Asia/Shanghai）。仓库为 PRIVATE，下载 Release 附件需要具备该仓库的 GitHub 访问权限。

实机系统：Windows 11 Home 中文版 `10.0.26200` / build `26200`，x64。其他 Windows 版本未实机验收，ARM64 不属于本次发布目标。NSIS `currentUser` 安装；缺少 WebView2 时，silent downloadBootstrapper 需要网络。

本记录明确区分早期候选 02 的全面原生覆盖与最终候选 03 的实际复验子集。02 的结果是对应实现的沿用证据，不冒称在 03 上全部重跑；安装、fixture、协议握手、原生界面及真实模型验收仍分别登记。历史 0.0.5 报告仅作来源说明。

## 最终候选 03 自动检查

本机证据根为 `D:\Codex\Atrio-WorkSpace-Windows-006\work\evidence`。

| 检查 | 状态 | 最终 03 证据 |
|---|---|---|
| npm ci / version | `PASS` | `final-03-npm-ci.log`、`final-03-version.log`：219 packages / 0 vulnerabilities，版本 0.0.6 |
| 共享前端 | `PASS` | `final-03-frontend.log`：8 文件 / 88 项 |
| Dev/Beta 前端与 NSIS | `PASS` | `final-03-frontend-dev.log`、`final-03-frontend-beta.log`、`final-03-dev-build.log`、`final-03-beta-build.log` |
| runtime/candidate fixture | `PASS` | `final-03-fixtures.log`：15 passed / 3 Darwin skips / 0 failed |
| Windows Rust 与编译 | `PASS` | `final-03-rust-tests.log`：120 passed / 0 failed / 3 expected ignored；`final-03-cargo-check.log` |
| 冻结与构建身份 | `PASS` | `final-03-freeze.log`、`final-03-candidate-check.log`、`final-03-artifacts.json`：main 提交、clean、内嵌版本/候选/指纹一致 |
| 发布源码 CI / macOS 回归 | `PASS` | [CI 37032940358](https://github.com/Jaime-Gu/atrio-agent-workspace/actions/runs/37032940358)：两平台前端、Windows MSVC、macOS arm64 编译及测试全部 SUCCESS |

macOS 初次 CI 的 `a_briefly_inherited_process_lock_is_retried_before_reporting_conflict` 超过目录锁 150ms 重试窗口，结果 102 passed / 1 failed；同一源码重跑 103 passed / 0 failed / 3 ignored。首次日志 `final-03-main-macos-first.log` 与通过日志 `final-03-main-ci-retry.log` 保留，未降低断言或忽略该测试。macOS runtime 仅作编译回归，不是发布包验收。

## 最终候选 03 原生与安装复验

| 检查 | 状态 | 最终 03 证据与范围 |
|---|---|---|
| Dev/Beta NSIS 安装 | `PASS` | `final-03-install.json`：各 exit 0，目录含中文与空格 |
| 安装资源完整 | `PASS` | `final-03-installed-dev-runtime.json`、`final-03-installed-beta-runtime.json`：各 28 文件 / 88,685,465 bytes，manifest/每文件哈希与锁一致 |
| 生产 EXE MCP stdio | `PASS`（synthetic Host） | `final-03-installed-dev-mcp.json`、`final-03-installed-beta-mcp.json`：各 11 项，`providerCalls=0` / `workspaceDatabasesOpened=0` |
| 打包前端与恢复 | `PASS` | `native-final-03/startup.txt`、`summary.json`：Dev `tauri.localhost`，4 模块、已保存文档、Planner/Dashboard 25%；Beta 打包窗口启动 |
| 渠道与内嵌身份 | `PASS` | `native-final-03/app-info.txt`、脱敏 summary：Dev/Beta identifier 分离，版本/候选/buildId/源码指纹与最终产物一致 |
| 持续 synthetic ACP 取消 | `PASS` | `native-final-03/cancel-processes.json`：应用拥有进程归零，真实模型调用 0；`session-summary-after-cancel.txt`：当前连接/session 均显示“未连接” |
| 运行中正常退出回收 | `PASS` | `native-final-03/exit-before-processes.json`、`exit-after-processes.json`：退出前 2，退出后 0，真实模型调用 0 |
| Beta 旧工作区副本 | `PASS` | `native-final-03/summary.json`：隔离完整备份副本打开；含私人内容的原始截图/日志不发布 |
| 原用户状态恢复 | `PASS` | `native-final-03/user-state-restored.json`：policy 与原备份哈希一致，测试 selection 移入忽略证据；无 SQLite 删除 |

最终 03 复验以 `native-final-03/summary.json` 为汇总；构建时 build.json 的初始 `acceptance=NOT_TESTED` 及早期 artifacts.json 的 pending 字段不是验收结果，实际状态由这份分项证据与不可变 Release 验收报告登记。

| 完整安装包 | 字节数 | SHA-256 |
|---|---:|---|
| `Atrio-WorkSpace-0.0.6-dev-windows-x64.exe` | 25,980,376 | `77ca4c3f0678ac39c936553ba26d909967b139b703a8c809df86d4a5aa251225` |
| `Atrio-WorkSpace-0.0.6-beta-windows-x64.exe` | 25,985,970 | `045c4201506bf4f957f938f88074df94e8272b93f82185f8329b9a1c56d401b7` |

## 早期候选 02 自动检查（沿用证据）

候选 `0.0.6-win-x64-02` 源码为 `3143c49dfbec91c37b1110722cdacc6104c22885`；以下表格只属于 02。

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

## 早期候选 02 全面原生与安装（沿用证据）

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
| 安装后生产 EXE MCP stdio | `PASS`（synthetic Host） | `candidate-02-installed-dev-mcp.json`、`candidate-02-installed-beta-mcp.json`，各 11 checks；`providerCalls=0`、`workspaceDatabasesOpened=0` |
| AppContainer 修复与旧工作区副本打开 | `PASS` | Windows 路径/containment 回归通过；`native-02/legacy-copy-open-summary.json`：完整备份副本原生打开，3 模块、无加载失败、未删除数据库 |
| 缺 adapter/CLI 明确错误 | `PASS`（GUI） | `native-02/missing-adapter-error.txt`、`missing-cli-confirmed.txt` / PNG：入口缺失要求完整安装，缺 Codex 宿主显示明确 CLI 路径原因并展开诊断，session/auth 为 not_checked，无 Mock fallback；隔离测试进程环境，模型与认证请求 0 |
| 未登录后的真实认证/会话失败 | `NOT_TESTED` | 本机官方 CLI `0.159.0-alpha.12.1` 为 `NotLoggedIn`，UI 提供官方登录说明；未执行真实 newSession 或登录 |

安装、fixture、握手、原生界面与真实模型调用分别登记，不能互相代替。旧工作区仅使用完整备份或隔离副本，不通过删除 SQLite 排除问题。

候选 02 验收后补充源码：连接汇总现在以 Host 当前状态显示“未连接/连接中/停止中/已连接”，历史 probe 标为“上次检查结果”，不伪报连接；新增 6 项共享前端回归（总计 88 项）、Windows 四来源字段与旧 manifest 兼容回归（候选 fixture 总计 15 passed / 3 Darwin skips）、synthetic ACP 与实际 Codex initialize-only harness。该补充已在最终 main 提交重新冻结为候选 03，未改写候选 02 身份。

本机 CLI 来源分别记录：原生 GUI 版本探测发现 `pi-node/current/codex.cmd` 的 `codex-cli 0.160.0`；initialize-only 明确使用官方 Desktop CLI `0.159.0-alpha.12.1`。两者均为外部用户安装，随包固定版本仅 ACP 1.13.1 / Node 22.23.3。

## Provider 范围与发布状态

- Hermes：`SKIPPED_BY_USER`，本机未安装。
- Claude：`NOT_TESTED`，用户暂不登录。
- Codex 真实认证/模型调用：`NOT_TESTED`，继续用户后置验收；没有复制认证或发送真实模型任务。

[#2](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/2) 已合并 `windows/integration`，[#3](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/3) 已合并 main。发布源码 `53e45575bfd6f0d8de7f1c1e5ada64862dfc859c` 与候选 03 安装包身份保持固定；发布后仅更新文档状态，不移动 tag、重建包或改写 02/03 清单。两份最终安装器 Authenticode 状态均为 `NotSigned`，SmartScreen 声誉未建立；UNC/网络映射盘/工作区内部 reparse 路径限制继续保留。
