# Atrio WorkSpace · 0.0.6 体验版本

> 接手与继续开发请先读 [持续更新 Handoff 主手册](/Users/user/Documents/Codex/2026-09-27/users-user-documents-codex-2026-09-2/outputs/pixel-workspace-handoff.md)。所有版本都是体验版本，按 `web → dev → beta` 验收与命名；每次落实迭代及时更新同一路径。

一个可在本地运行的个人 Agent 工作区初版。用可移动、缩放和聚焦的模块组织对话、计划、Markdown 文档与数据概览，验证「描述目标 → Agent 提议 → 用户审批 → 保存成果」的完整交互。

可选择本地 Mock、Hermes、Claude Code 或 Codex ACP 入口。Hermes 与 Claude Code 已完成真实模块工具预验收；Codex 的ACP实现和已知路径保留，真机验证按用户决定后置。连接失败不会静默回退Mock。

当前源码版本为 0.0.6，左上品牌 Atrio；现有 Bundle ID 和数据目录保持兼容。本轮页面信息简介化候选为 `0.0.6-information-03`，按 Dev → Beta 验收，未生成 0.0.6 Web 候选。当前能力、固定依赖和 Workspace MCP 说明见 [0.0.5说明](docs/ATRIO_005.md)，历史 0.0.5 候选记录与最新渠道验收见固定 Handoff。

## 连接 Hermes

1. 先在自己的终端确认 `hermes --version`、`hermes acp --check` 和模型认证可用。本机实测入口为 `~/.local/bin/hermes`，版本 0.21.3；软件不内置安装器或登录器。
2. 在独立工作区的设置选择「Hermes · 本机 ACP」，保存命令路径。参数固定为 `acp`，不会拼接 shell 字符串。探测可分别显示未安装、依赖异常和可用状态；探测成功不等于认证已验证。
3. 左下盾牌选择有效权限。询问模式须先明确接受 Hermes 的本机运行范围；受限/只读未能证明覆盖 Hermes 自有工具，因此不允许该模式连接。完全访问由用户明确选择，不绕过 Host 文件校验或操作系统权限。
4. 点击连接，成功后发送消息。正常多轮复用 provider session；取消、断开或权限变更会回收本应用管理的进程，之后重新连接创建新会话。重启恢复本地聊天历史，不声称恢复原 provider 会话。

Hermes 的 cwd 不是沙箱，可能使用自己的文件/命令/网络工具并加载自身既有记忆。Host 只控制经它处理的操作和 Hermes 实际发出的 ACP permission request。工具状态通知只记录和展示，不会再执行一次。真实 Hermes/Claude Code 可通过Workspace MCP读取、提议并经Host修改Document、Planner、Dashboard和公共标题/布局；Codex复用同一实现但本轮未做真实调用验收。

工作区权限为「禁止运行 / 受限 / 询问 / 完全访问」，设置页系统策略为「按工作区 / 全部允许 / 全部不允许」。系统覆盖保留原局部偏好；有效权限变更（含提权）会终止旧运行并使旧 Agent 审批失效。禁止 Agent 不阻止用户手动编辑和布局。复制工作区不会继承本机局部信任，Dev/Beta 系统设置分开。

## 验收与数据保护

0.1.3 起有渠道单实例和物理工作区进程锁、旧写入者检查，以及文件/数据库分步提交的恢复记录。旧数据须先完整备份；SQLite 含不可重建的消息、会话、审批和审计，不能作为缓存删除。备份位置、候选清单、已测与后置的真实用户项目见固定 Handoff。

0.1.4 源码的真实 Hermes 预验收已验证随机口令多轮、取消回收和撤权；最终安装包验收以 Handoff 的候选记录为准。测试使用私有隔离 Hermes profile，不包含原记忆/技能/历史；通过 Host 启动环境 `PIXEL_HERMES_TEST_HOME` 指定，缺失目录会拒绝回退个人 profile。普通运行不启用该测试覆盖。该私有目录可能包含 Hermes 自行生成的认证配置备份，严禁打入交付归档。

## 当前可以做什么

- 在 24 列画布上创建、重命名、复制和移除四类内置模块；拖动、缩放、键盘移动与聚焦。
- 接收 Mock 流式回复，审批新模块提议，观察运行状态与事件。
- 在计划中添加任务、切换完成状态，查看实时完成率。
- 阅读和编辑 Markdown；手动保存与 Agent 修改均先生成审批，批准后才写入已有文档。
- 在桌面版选择本地工作区，用 Markdown、JSON 和 SQLite 保存工作内容、布局、会话与审批。
- 演示拒绝修改、版本冲突、受限模式、任务失败、取消与重启恢复。

0.1.1 修复工作区切换时的快照串用和迟到操作、页面切换丢失草稿、任务标题长度不一致、搬移工作区后 Agent 目录失效，以及 DTO 契约测试的格式依赖。编辑草稿在本次应用会话内按工作区和模块保留；退出应用或刷新网页后，尚未提交的草稿不会恢复，已提交审批仍由 Host 持久化。

周活跃柱形图使用明确标注的「演示数据」；任务、模块数量与最近事件来自当前工作区。

0.1.2 统一左上角、侧栏、设置和标题的版本来源。Web 使用构建版本，原生使用正在运行的 Host 包版本；设置页显示实际应用路径和数据目录。版本校验失败会明确显示“版本未确认”，不会用写死的文字冒充当前包。

## 运行源码

以下命令在项目根目录执行。

### 浏览器交互预览

准备 Node.js 22.12 或更新版本与 npm：

```sh
npm ci
npm run dev
```

热更新预览地址固定为 `http://localhost:1421`。构建候选验收使用第二个端口：

```sh
npm run candidate:freeze -- 0.0.5-atrio-01
npm run candidate:build -- web work/candidates.noindex/0.0.5-atrio-01/candidate.json
npm run preview
```

构建预览地址固定为 `http://localhost:1422`，使用候选独立冻结目录，两个入口都显示 `0.0.5-web`。服务只绑定 localhost，端口占用时直接报错，不自动跳号。

浏览器预览把状态保存到当前站点的 `localStorage`，键为 `pixel-workspace-preview-v1`。它不访问本地文件，不运行 Rust Host，也不创建 SQLite 数据库；界面中的文档路径只是演示值。更换端口或清除站点数据，会看到独立或重置后的预览工作区。

### 原生桌面开发

在 macOS 上准备 Xcode Command Line Tools，以及自己安装的 Rust stable / Cargo。建议使用当前稳定版 Rust，以满足锁定依赖的编译要求。本项目不需要也不提供机器专属 Rust 路径。

```sh
npm ci
npm run desktop:dev
```

脚本加载可用的 Rust 工具链，Tauri 自动启动 1420 的 Vite，并打开 `Atrio WorkSpace Dev`；无需另起同端口服务。Dev 使用 `dev.pixel.workspace.dev` 和独立的应用数据目录。兼容命令 `npm run tauri -- dev` 也会自动附加 Dev 配置，避免误用 Beta 身份。

构建可独立运行的 Dev 应用：

```sh
npm run desktop:dev:build
```

Web 与 Dev 的相关验收通过后，构建 Beta `.app` 与 `.dmg`：

```sh
npm run desktop:beta:build
```

脚本构建 Apple Silicon 版本，并将本次 `.app` 放入 `work/builds.noindex/` 下独立目录，避免临时构建副本进入 Spotlight。最近产物路径写入该目录的 `latest-dev.json` / `latest-beta.json`；Beta DMG 命名为 `Atrio-WorkSpace-arm64-0.0.5-beta.dmg`。发布验收使用 `candidate:build` 绑定同一冻结来源，详见 docs/CANDIDATES.md。

基础版本来自 package.json，构建前 `npm run version:check` 核对 npm lock、Cargo、Tauri 与渠道配置。最低 macOS 版本仍为 12.0，使用 ad-hoc 签名；不是已公证的正式发布。测试范围见 [验收记录](docs/ACCEPTANCE.md)。

## 十分钟走通一次协作

1. 保持「Mock Agent」与「逐次询问」模式，在底部输入 `创建一个本周计划`。等待流式回复与模块提议，点击「批准并创建」。画布出现计划模块。
2. 勾选一个任务，再添加自己的任务。观察模块内进度与数据看板的完成率变化。
3. 拖动模块标题改变位置，用右下角调整大小。双击标题聚焦，按 `Esc` 回到画布。选中模块后也可用方向键移动，`Shift + 方向键` 调整尺寸。
4. 选中已有文档，输入 `修改文档`。审批面板显示原文和修改后内容；先「拒绝」，确认内容不变。再发送相同请求并批准，确认新内容出现。桌面版同时更新文档文件。
5. 点击文档里的「编辑」，直接修改 Markdown 后「保存」。保存同样生成审批；批准后应用，拒绝后仍可在编辑器中看到本次草稿。
6. 在左下工作区权限切到「受限 / 只读」，再次用 Mock 请求修改。可以查看提案并拒绝，Host 会阻止 Agent 写入。有效权限改变会撤销旧 Agent 提案；切回询问后请重新提交任务。
7. 输入 `运行一个长任务`，流式输出时点击输入框右侧的停止按钮。查看状态变为「任务已取消」，确认可继续提交新任务。
8. 输入 `演示一次任务失败`。结束后在运行记录里查看失败事件，然后输入一个新目标，检查仍可继续工作。
9. 桌面版退出再打开，检查模块、布局、已保存任务和文档恢复；浏览器版可用刷新验证当前站点预览状态的恢复。

也可以输入 `创建一个文档`、`创建数据看板`。这些指令演示固定行为；Mock 不理解任意复杂任务，也不会真的根据自然语言定制日程或执行代码。

快捷键：`⌘/Ctrl + K` 打开快速查找；`⌘/Ctrl + J` 聚焦输入框；输入框 `Enter` 发送、`Shift + Enter` 换行。只有选中模块卡片本身时，方向键才控制画布，表单内仍按正常编辑行为处理。

## 本地数据在哪里

桌面版第一次启动使用 Tauri 应用数据目录下的 `Workspace/`。在 macOS，通常为：

```text
~/Library/Application Support/dev.pixel.workspace/Workspace/
```

上面是 Beta 目录，兼容历史体验数据。Dev 默认使用 `~/Library/Application Support/dev.pixel.workspace.dev/Workspace/`。当前仍未实现跨进程工作区锁，即使两渠道默认目录不同，也不要让两个实例手动选择同一工作区。

「设置与连接 → 选择工作区目录」可切换工作区。最后选择的目录记录在应用数据目录的 `workspace-selection.json` 中，下次启动继续使用。以界面显示的工作区绝对路径为准。

切换期间暂停提交动作，旧工作区运行会取消并保存。每个原生动作同时携带当前根目录，Host 会拒绝迟到的旧工作区操作。Agent 工作目录现保存为相对路径（留空表示根目录，兼容 0.1.0；输入 `.` 也会规范化为空值）；旧绝对路径可依据 SQLite 中记录的原根目录安全迁移。搬移旧格式工作区时请保留整个 `.workspace/`，缺失旧根记录时不会猜测并放行外部路径。

工作区内部结构：

```text
你的工作区/
├── notes/
│   └── <module-id>.md           # 文档正文
└── .workspace/
    ├── workspace.json          # 模块索引、权限、Agent 描述等
    ├── modules/
    │   └── <module-id>.json     # 模块配置、任务与布局
    └── workspace.sqlite3       # 会话、审批、快照索引与审计事件
```

SQLite 运行中还可能生成 `workspace.sqlite3-wal` 与 `workspace.sqlite3-shm`。备份时先正常退出应用，再复制整个工作区。仅复制 `notes/` 可保留文档正文；要保留会话、审批与完整运行历史，还需要 `.workspace/`。

Markdown 文件是文档正文的来源。每次 Host 请求快照时会重新读取文件；本版尚无文件监听，外部编辑不会立即主动推送到界面。审批绑定原文的 SHA-256 revision，外部更改或删除原文会使旧审批失效，避免直接覆盖。此时拒绝旧审批，再生成修改；新的 Host 快照会重新读取文件。

「移出画布」只移除模块关联，保留其文档文件。文件页当前只列出已关联文档，不提供完整目录索引或全文搜索。

## 权限与当前范围

- 原生 Rust Host 决定审批与文件写入。前端只提交动作和审批 ID，不能自行提供一份「已批准」写入内容。
- Agent 创建模块与修改文件需要逐次审批；手动编辑已有文档也需要审批。用户直接创建普通模块不额外弹审批；创建或复制文档会受写入策略约束。
- 受限模式允许 Mock 生成修改预览，禁止应用 Agent 审批；用户仍可手动新建、编辑、保存文档和调整布局。它不是整个工作区只读，也不是 Hermes 自有工具的沙箱。
- 文件入口限制在选定工作区，检查 `..`、绝对路径、符号链接越界及 revision。单份文档上限为 1 MB，模块上限为 100。
- `stdio` 当前只接已安装 Hermes：探测运行版本与 ACP 依赖检查，明确连接后才启动服务和会话。不要在描述符填入 API key 或 token；认证由 Hermes 自己管理。

未实现：通用 Host 终端和网络工具、文件删除、文件监听、全文索引、插件市场、并行 Agent、跨设备同步、provider resume/list 。运行中的任务退出后不会自动续跑；本地文件审批可恢复，已经退出的 Agent 权限请求不会重放。

浏览器后端仅用于体验前端交互；原生 Host 的文件隔离、SQLite 审计与权限校验，需要以原生实现及其测试为准。

## 检查与继续开发

```sh
npm ci
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

Rust 的共享 DTO 测试通过 Node.js 调用 TypeScript 编译器检查实际序列化结果，需要先安装 npm 依赖；缺少依赖时会明确失败。前端生命周期回归测试使用 jsdom 和真实 React 组件，不等同于原生窗口人工验收。

- [架构说明](docs/ARCHITECTURE.md)：前端、Host、持久化与审批流程。
- [验收记录](docs/ACCEPTANCE.md)：已完成检查、测试覆盖和待执行的原生 UI 验收。
- 前端入口：`src/App.tsx`；组件：`src/components/`。
- 原生入口与事件桥：`src-tauri/src/lib.rs`；本地工作区内核：`src-tauri/src/kernel.rs`。

### Codex included in the 0.0.5 candidate 09 macOS package

The arm64 DMG includes only the Codex ACP adapter and pinned Node runtime; the recipient’s official Codex CLI remains external. Keep the default `codex-acp` command in settings. ACP is ready automatically when the App is installed; click Connect to check it and reuse the local Codex login. No Atrio account form, login helper or npm setup is required. If Codex is not signed in, sign in with the official Codex setup and retry. The software version remains `0.0.5-beta`; candidate/build IDs distinguish this package from earlier candidates. See [bundled runtime details](docs/CODEX_BUNDLED.md). No credentials or personal configuration are shipped, and live Codex inference remains untested locally by user request.
