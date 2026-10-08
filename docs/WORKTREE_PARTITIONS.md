# Atrio WorkSpace 编码分区规划

修订日期：2026-10-08。目标显示版本：`0.0.7[1]-web/dev/beta`。本次准备只登记分支和文件职责，产品代码保持线上 0.0.6。

## 基线与分支

仓库：`Jaime-Gu/atrio-agent-workspace`。线上标签 `0.0.6` 对应提交：

```text
53e45575bfd6f0d8de7f1c1e5ada64862dfc859c
```

`backup/0.0.6-online` 指向该现有提交，无需创建空提交。`chore/worktree-partitions-0071` 在基线上增加协作说明；其准备提交为所有角色的共同起点。发布标签和现有主分支保持当前状态。

| 角色 | GitHub 分支 | 任务卡 | 初始状态 |
| --- | --- | --- | --- |
| 原始备份 | `backup/0.0.6-online` | 原始提交只读 | 基线保存 |
| 集成 | `feature/0.0.7-1-rail` | [integration](worktrees/roles/integration.md) | 等待分区提交 |
| 契约 | `feature/0.0.7-1-contracts` | [contracts](worktrees/roles/contracts.md) | 可以定义契约 |
| 前端 | `feature/0.0.7-1-ui-rail` | [ui-rail](worktrees/roles/ui-rail.md) | 等待契约 |
| Host | `feature/0.0.7-1-host-layout` | [host-layout](worktrees/roles/host-layout.md) | 等待契约 |
| 验收 | `feature/0.0.7-1-validation` | [validation](worktrees/roles/validation.md) | 等待完整集成提交 |

Git worktree 位于开发者本机，每个工区仍有完整仓库；读取范围由任务需要控制。一个阶段内每个文件只有一个写入者。废弃本机 0.0.7、历史候选和日志不进入本轮，不需要寻找本机 `0.0.6-information-03` 归档。

在开发者自己的仓库创建工区，目录名称可按本机选择；同名现存目录或分支须先核对归属，不能覆盖：

```sh
git fetch origin --tags
git rev-parse '0.0.6^{commit}'
git worktree add -b local/0071-integration ../atrio-0071-integration origin/feature/0.0.7-1-rail
git worktree add -b local/0071-contracts ../atrio-0071-contracts origin/feature/0.0.7-1-contracts
git worktree add -b local/0071-host ../atrio-0071-host origin/feature/0.0.7-1-host-layout
git worktree add -b local/0071-ui ../atrio-0071-ui origin/feature/0.0.7-1-ui-rail
git worktree add -b local/0071-validation ../atrio-0071-validation origin/feature/0.0.7-1-validation
```

本机分支与对应远端分支登记在任务卡。一个分支只能由一个 worktree 使用；这些命令不启动服务或安排 Codex 对话。

## 文件所有权

### contracts

唯一写入：`src/lib/types.ts`、`src/lib/layout.ts`、`src/lib/layout.test.ts`、`scripts/check-rust-contract.mjs`、对应新增 `scripts/check-rust-contract.test.mjs`、新增契约说明 `docs/worktrees/CONTRACTS_0071.md`。

确定具体字段、动作信封、命令、参数、返回类型、错误类别和哈希规则。Rust DTO、动作及内嵌契约测试均位于 `kernel.rs`，由 host-layout 独占，contracts 只读。几何计算、旧字段回填、顺序校验和置顶纯函数集中于 layout；其他前端组件通过该模块调用。

### ui-rail

唯一写入：

- `src/App.tsx`、`src/App.test.tsx`
- `src/components/Canvas.tsx`、`TaskRail.tsx`、`AnchoredMenu.tsx`、`ModuleContent.tsx`、`Panels.tsx`、`PolicyPanel.tsx`、`ProposalReview.tsx`、`Dialog.tsx`、`ModuleErrorBoundary.tsx`
- 上述组件在相同目录对应的 `.test.tsx` 文件
- `src/styles.css`、`src/components/module-content.css`
- `src/lib/api.ts`、`src/lib/api.test.ts`
- `src/lib/preview.ts`、`src/lib/preview.test.ts`、新增 `src/lib/preview-layout.test.ts`

TaskRail、AnchoredMenu 在原标签中不存在，属于预定新增文件。api 已有通用 dispatchAction，先复用；偏好接口按契约接入。权限组件只调整视觉和入口，不能修改权限语义。Provider、版本、依赖和 Host 文件只读。

所有 UI、DOM 与样式作为同一依赖组，左侧 44px 工具条和底部最大 620px 输入框均为独立悬浮控件，不占 App 固定网格行列；精确几何、圆角、阴影与内容层次遵守同一 rail 参考和产品 Prompt。integration 提供可访问参考并记录哈希；缺少原始视觉参考时不能宣称复刻完成。现有紫色主题与真实数据保持。

### host-layout

唯一写入：`src-tauri/src/kernel.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/module_tools.rs`，以及预定新增 `src-tauri/src/ui_preferences.rs`、`layout_stack.rs`、`ui_preferences_tests.rs`、`layout_stack_tests.rs`。内嵌 Rust DTO、序列化测试和恢复测试归同一负责人。

负责 Host 置顶动作、manifest、SQLite snapshot、pending_commit、旧字段回填、幂等记录和独立渠道 UI 偏好。Web localStorage 实现归 ui-rail。可只读 `policy.rs`、`locks.rs`、`platform/` 与契约文件，禁止修改。新增其他辅助文件先登记准确路径。

纯用户置顶沿用可信 user 手动布局语义；Agent 禁止运行不能阻止用户选择模块，不改变 Agent 提案权限或动作来源。Provider 和认证只读。

### integration

唯一写入：

- `package.json`、`package-lock.json`
- `src-tauri/Cargo.toml`、`Cargo.lock`、`build.rs`
- `src-tauri/tauri.conf.json`、`tauri.dev.conf.json`、`tauri.windows.conf.json`
- `src-tauri/src/identity.rs` 及其内嵌测试
- `src/lib/app-info.ts`、`src/lib/app-info.test.ts`、`vite.config.ts`、`tsconfig.json`、`index.html`、`src/main.tsx`
- `scripts/` 内除 contracts 契约检查脚本及其对应测试外的构建、候选、版本、平台资源脚本和测试
- 明确包含 `stage-macos.mjs`、`tauri.mjs`、`candidate-lib.mjs`、`candidate-lib.d.mts`、`codex-runtime.lock.json`、`package-codex-runtime.mjs`、`scripts/runtime/`、`scripts/windows/`
- `.github/`、`AGENTS.md`、`CONTRIBUTING.md`、`README.md`、`.gitignore`、`.gitattributes`、`.worktreeinclude`、`windows-provenance.json`
- `docs/` 中除 contracts 的 `CONTRACTS_0071.md` 外的协作、接口索引和验收说明

integration 合并成果与维护共享文件，不能代写其他分区产品功能。产品冲突退回原负责人在自己的分支提交修复；确认提交后再合并。候选和 build manifest 通过现有工具生成，验收说明由 integration 接收 validation 结果后更新。

### validation

产品源码和所有 Git 跟踪文件只读。证据写入自己被忽略的 `work/validation/<集成提交>/`。新增跟踪测试归原产品负责人，公共文档/CI 归 integration。验收不能直接读取其他对话未提交工作。

### 默认只读

未登记文件没有写入权。`policy.rs`、`locks.rs`、`platform/`、`agent/`、`mcp_bridge.rs`、`workspace_tools.rs`、图标和 capability 本轮只读。需要修改时提交路径、理由和验证范围，由 integration 更新清单并指定一个负责人，再开始修改。

## stackOrder 与 revision

stackOrder 使用每模块独立整数，正常顺序为 1..N，空工作区为零个模块；数值越大越靠前。manifest、快照与恢复记录保存该字段。

- moduleRevision 保持线上 0.0.6 哈希输入，包括内容、任务、标题、几何布局、文档 revision、看板配置，不加入 stackOrder。
- workspace_module_revision 保持既有工作区身份、allowOverlap、模块 ID 和 moduleRevision 的规范化输入，不加入 stackOrder。
- 置顶只更新独立字段，保持 `state.modules` 数组及 manifest 模块 ID 顺序，避免数组重排改变工作区哈希。UI 按 stackOrder 计算 z-index。
- 纯置顶更新顺序、持久快照和 `layout/stack_order_updated` 审计；内容 revision、审批、提案和任务运行状态不变。应用较早创建的 Agent 提案时保留用户当前层级。
- 拖动/尺寸调整仍修改 x/y/w/h 与原有 revision，同时保存当前层级。不得通过 set_layouts 伪装纯置顶。
- 全部旧字段缺失时，统一按原模块 ID 顺序回填；创建、复制、移除通过受控动作维护连续序列。部分缺失、重复、非法整数或未知模块属于损坏数据，直接返回错误。
- 动作携带 moduleId、真实 moduleRevision 和 operationId；root/generation 经现有 IPC 信封。Host 在锁内检查，已在顶部时不改数据，重复 operationId 不重复应用。
- 前端交互后立即呈现顺序意图，串行向 Host 提交，发送时取得最新已确认 revision。失败撤销对应未确认意图并展示错误，工作区切换丢弃旧代次响应；后台内容更新不能覆盖新的未确认层级。

契约说明定义相同行为，Rust Host 与 Web preview 分别实现并执行相同数据场景。

## 依赖与合并

1. integration 准备共同提交 P，协作文档与产品依赖准备单独登记，原始备份分支仍指向标签提交。
2. contracts 基于 P 提交类型与纯函数，integration 合并得到契约提交 C。
3. host-layout 和 ui-rail 各自合并已确认 C 后可以并行实现，不能沿用工区创建时的过期接口。
4. Rust 配套完成后运行完整跨语言测试。contracts 独立阶段只报告可执行局部检查，不虚报整体契约通过、不使用临时替代类型。
5. 分区提交注明修改路径、依赖提交和实际测试。integration 先合并 Host，再合并 UI，运行完整检查。新的接口变化仍交回 contracts。
6. integration 固定完整提交 I，从干净目录冻结、构建候选。validation 在自己的 worktree 执行 `git merge --ff-only <I>`，使验收分支精确指向 I；无法快进时停止并报告，不产生额外合并提交，也不把验收分支合并到 integration。核对提交和产物来源后验收。
7. 缺陷退回原负责人，经新提交合并后创建新候选，复验受影响项；不改旧 manifest。Web、Dev、Beta 绑定同一候选。

## 运行隔离

- 各工区分别执行 npm ci，分别管理 node_modules、work 与 Rust 编译目录，不能共享可写目录。
- Cargo 默认使用本工区 src-tauri/target；检查外部 CARGO_TARGET_DIR，发现共享路径改为本工区专用路径。构建产物位置按实际脚本核对。
- 集成阶段固定 1420/1421/1422 归 integration；验收阶段释放后显式交给 validation，不能同时启动或终止未知服务。
- worktree 不隔离 Bundle ID、SQLite、IPC、应用设置和锁。同机原生 Dev/Beta 只运行一个验收执行者，按渠道顺序检查，备份/恢复渠道设置并使用独立 A/B/A-copy 工作区。
- Host 分区只运行自己专用真实目录与数据库的自动测试，不启动用户日常 GUI。中间结果位于被忽略的 work，不使用 /tmp。
- 运行时二进制不会随 Git 自动出现；integration 按原平台锁准备、校验 macOS/Windows 资源。不能省略哈希、许可证或签名校验。
- 凭据、个人 profile、用户工作区禁止进入 Git 或 .worktreeinclude；非敏感开发配置逐项登记复制用途。

## 检查与交付

contracts 检查类型、布局纯函数和脚本；Host 检查真实目录/SQLite/manifest、兼容、幂等、revision 和恢复；UI 检查真实组件、菜单、焦点、悬浮布局、即时置顶和 Web 保存。

integration 在完整提交执行：

```sh
npm ci
npm test
npm run format:check
npm run version:check
npm run build
npm run build:dev
npm run build:beta
npm run candidate:test
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
git diff --check
```

validation 验证 rail 同尺寸参考、菜单锚点/完整按钮、A/B/A 层级、退出恢复、任务栏展开收起、窄窗口、Dev 真实 Host 和 Beta 独立安装启动。未执行结果写 NOT_TESTED，原生平台结果分别登记。

每个对话开头填写：

```text
角色与任务卡：
本机/远端分支与 worktree 路径：
起点提交与已取得契约提交：
允许写入路径及只读依赖：
当前目标及预期行为：
测试命令与证据目录：
成果提交与未测试项目：
```

提交前核对路径没有越界、接口提交已取得。分区规则属于协作约定；如需操作系统强制文件隔离，另行配置。

完成后保留备份、角色成果、集成提交与发布证据。只清理已确认合并且没有未提交内容的任务工区，其他活动分支与标签保留。修复使用文件编辑工具和新提交，不执行 Git 回滚命令。
