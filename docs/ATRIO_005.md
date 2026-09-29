# Atrio WorkSpace 0.0.5

软件版本从0.1.4重设为0.0.5是明确的产品决定，不降低数据库schema或清理旧数据。显示品牌为Atrio，Dev/Beta应用名为Atrio WorkSpace；包名、可执行文件、Bundle ID、应用数据目录、localStorage及锁标识保持不变。

## Provider与本机依赖

| Provider | 已测试入口 | 宿主 | 本轮状态 |
| --- | --- | --- | --- |
| Hermes | `hermes acp`，0.21.3 | 已安装Hermes | 真实MCP读写预验收通过，冻结包结果见Handoff |
| Claude Code | `claude-agent-acp`，官方npm `@agentclientprotocol/claude-agent-acp@0.81.2` | Claude Code 2.1.278 | 真实MCP读写预验收通过，冻结包结果见Handoff |
| Codex | `codex-acp`，官方npm `@agentclientprotocol/codex-acp@1.13.1` | 已发现CLI 0.155.0-alpha.16.4 | ACP代码与配置已接入；真实调用按用户最新指令后置 |

开发安装的固定adapter目录：`~/.local/share/atrio/acp-adapters/0.0.5/node_modules/.bin/`。应用从本机查找该目录或用户指定的绝对入口；不内置下载/更新器。其他机器需自行安装相同官方包并配置命令路径。Codex宿主路径记录为`/Applications/ChatGPT.app/Contents/Resources/codex`，Claude为`~/.local/bin/claude`。已安装宿主/adapter、认证、握手、会话可用分别报告。

Codex当前路由曾返回`403 native codex clients only`；没有改客户端身份绕过限制。用户已要求只保留ACP与目录配置、跳过本机真机验证，不将其记为功能实测通过。

## Workspace MCP

同一个已签名应用可执行文件以`--workspace-mcp`启动轻量stdio服务，不启动GUI、不打开工作区SQLite、不直接写文档。服务转发到Host私有Unix socket；目录0700、socket0600，socket名无凭据。Host始终持有唯一工作区锁并串行处理工具。

五个工具：`workspace_list_modules`、`workspace_read_module`、`workspace_get_selection`、`workspace_propose_changes`、`workspace_get_proposal_result`。协议为MCP逐行JSON-RPC；session/new显式注入绝对可执行路径及结构化参数。列表仅返回摘要和revision；读取仅针对指定非对话模块。

会话token只在内存和MCP启动环境中；每轮独立runScope由Host签发并放入本轮上下文。每个请求必须显式携带该scope，不把迟到调用套入新run。Host核对workspace/root/generation/provider/runtime/run/permissionEpoch，并为IPC请求和幂等操作分别记录ID。取消、断连、撤权或切换使旧scope和待审批提案失效。凭据回显在持久化前脱敏，不能作为模块内容提交。

提交修改返回`pending`、`applied`等真实结果；Ask等待用户界面审批，Agent下一轮可查询结果。pending不是成功；不同Provider不能查询/复活彼此的提案。工具请求不会等待用户审批而占住ACP线程，UI、取消和撤权仍可操作。

## 类型化模块变更

- Document：Host读取before，批准时核对文件和模块revision，写入Markdown并刷新模块。
- Planner：完整任务列表差异，新增/修改/移除/完成状态/时间字段；Host生成缺失ID并校验标题、数量和唯一性。
- Dashboard：限定真实本地指标与规划模块筛选，不允许SQL、HTML/JS或外部数据源；进度来自实际任务。
- Metadata：模块标题及明确提出的布局修改；Host校验尺寸和碰撞，Full也要求明确审批布局提案。
- Create：四种内置类型的受控创建，新建文档及模块元数据沿用已有恢复日志。

模块revision覆盖内容、任务、标题、布局、文件revision和看板配置，排除瞬态运行状态。批准时再次核对；外部/用户编辑产生冲突，幂等键不能重复写入。软件版本降号与数据schema无关；模块新字段使用兼容默认值，已有内容不批量改品牌。

用户未保存草稿不自动送给Agent或写成磁盘正文。选择模块只传递ID和可读范围，由Agent实际工具读取已保存内容；不自动传全部聊天、凭据或其他工作区。

## 权限边界与测试隔离

沿用系统三态和工区四态；授权确认绑定Provider，旧Hermes确认不能给Claude/Codex授权。自有工具只读未验证的Provider在restricted下拒绝连接。MCP受控不代表外部Agent全部文件/命令工具被沙箱化。

真实测试使用独立合成工作区及Provider隔离profile。Hermes只引用既有模型配置且不带原记忆；Claude仅将认证/路由白名单传入子进程内存，禁用个人hooks/plugins/记忆；Codex预留隔离配置，不再运行真实测试。所有profile可能含运行时认证材料，禁止归档。诊断不打印环境、原始认证配置或完整子进程argv。

发布按同候选Web→Dev→Beta验收，结果与Codex后置例外以固定Handoff及outputs报告为准。旧P0指针/最小窗口体验由用户继续后置，未标PASS。
