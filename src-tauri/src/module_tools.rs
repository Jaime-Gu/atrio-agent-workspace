//! Typed local workspace tools. This is part of Kernel, not a second database
//! writer: the MCP sidecar forwards requests here and cannot select a root or actor.
use super::*;
use serde_json::{json, Value};

const MAX_PROPOSALS: usize = 2_000;
const PROPOSAL_TTL_SECONDS: i64 = 30 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DashboardMetric {
    TasksDone,
    TasksTotal,
    ModuleCount,
    DocumentCount,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DashboardConfig {
    pub metrics: Vec<DashboardMetric>,
    #[serde(default)]
    pub planner_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}
impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            metrics: vec![
                DashboardMetric::TasksDone,
                DashboardMetric::TasksTotal,
                DashboardMetric::ModuleCount,
                DashboardMetric::DocumentCount,
            ],
            planner_ids: Vec::new(),
            title: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskInput {
    #[serde(default)]
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ModuleChange {
    Document {
        module_id: String,
        content: String,
    },
    Planner {
        module_id: String,
        tasks: Vec<TaskInput>,
    },
    Dashboard {
        module_id: String,
        config: DashboardConfig,
    },
    Metadata {
        module_id: String,
        title: Option<String>,
        layout: Option<Layout>,
    },
    Create {
        module_type: ModuleType,
        title: String,
        content: Option<String>,
        tasks: Option<Vec<TaskInput>>,
        dashboard: Option<DashboardConfig>,
    },
}
impl ModuleChange {
    fn target(&self) -> Option<&str> {
        match self {
            Self::Document { module_id, .. }
            | Self::Planner { module_id, .. }
            | Self::Dashboard { module_id, .. }
            | Self::Metadata { module_id, .. } => Some(module_id),
            Self::Create { .. } => None,
        }
    }
    fn changes_layout(&self) -> bool {
        matches!(
            self,
            Self::Metadata {
                layout: Some(_),
                ..
            }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProposalInput {
    request_id: String,
    idempotency_key: String,
    expected_revision: String,
    change: ModuleChange,
}

/// Durable result and immutable creation identity. No session token or run scope
/// is serialised here. Prepared after-state is Host-produced, never Agent-supplied.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleProposal {
    pub id: String,
    pub request_id: String,
    pub idempotency_key: String,
    pub fingerprint: String,
    pub provider: String,
    pub context: ConnectorContext,
    pub status: String,
    pub title: String,
    pub module_id: String,
    pub expected_revision: String,
    pub result_revision: Option<String>,
    pub summary: String,
    pub created_at: String,
    pub updated_at: String,
    pub change: ModuleChange,
    pub before: Option<WorkspaceModule>,
    pub after: WorkspaceModule,
}

pub(super) struct ModuleToolLease {
    token: String,
    provider: String,
    session: ConnectorContext,
    run: Option<(String, ConnectorContext)>,
}

pub fn module_revision(module: &WorkspaceModule) -> String {
    // Status is transient presentation state; all user-editable metadata counts.
    content_revision(&json!({"id":module.id,"type":module.module_type,"title":module.title,
        "layout":module.layout,"tasks":module.tasks,"content":module.content,
        "filePath":module.file_path,"revision":module.revision,"dashboardConfig":module.dashboard_config}).to_string())
}

impl Kernel {
    pub(super) fn redact_tool_credentials(&self, text: &str) -> String {
        let mut clean = text.to_owned();
        if let Some(lease) = &self.module_tool_lease {
            clean = clean.replace(&lease.token, "[tool-session-redacted]");
            if let Some((scope, _)) = &lease.run {
                clean = clean.replace(scope, "[run-scope-redacted]");
            }
        }
        clean
    }
    pub(super) fn redact_stream_delta(&mut self, text: &str, terminal: bool) -> String {
        let pending = self
            .active
            .as_mut()
            .map(|run| std::mem::take(&mut run.pending_output))
            .unwrap_or_default();
        let combined = self.redact_tool_credentials(&format!("{pending}{text}"));
        let mut suffix = 0;
        if let Some(lease) = &self.module_tool_lease {
            for secret in std::iter::once(lease.token.as_str())
                .chain(lease.run.as_ref().map(|(scope, _)| scope.as_str()))
            {
                for length in 1..secret.len() {
                    if combined.ends_with(&secret[..length]) {
                        suffix = suffix.max(length);
                    }
                }
            }
        }
        let cut = combined.len() - suffix;
        if terminal {
            if suffix >= 8 {
                format!("{}[scope-fragment-redacted]", &combined[..cut])
            } else {
                combined
            }
        } else {
            if let Some(run) = self.active.as_mut() {
                run.pending_output = combined[cut..].into();
            }
            combined[..cut].into()
        }
    }
    pub fn configure_tool_bridge(&mut self, socket: PathBuf, executable: PathBuf) {
        self.tool_bridge_paths = Some((socket, executable));
    }
    pub(super) fn module_tool_mcp_servers(&self) -> Vec<Value> {
        let Some((socket, executable)) = &self.tool_bridge_paths else {
            return Vec::new();
        };
        let Some(token) = self.module_tool_session_token() else {
            return Vec::new();
        };
        vec![
            json!({"name":"atrio-workspace","command":executable.to_string_lossy(),"args":["--workspace-mcp"],
            "env":[{"name":"ATRIO_MCP_SOCKET","value":socket.to_string_lossy()},
                {"name":"ATRIO_MCP_SESSION_TOKEN","value":token}]}),
        ]
    }
    pub(super) fn module_tool_prompt_text(&self, text: &str, context: &ConnectorContext) -> String {
        if self.state.agent.transport == "mock" {
            return text.into();
        }
        let Some(scope) = self.module_tool_run_scope() else {
            return text.into();
        };
        format!("{text}\n\n[Host workspace context]\nThis turn's workspace tool runScope: {scope}\nSelected module ID: {}. Only saved content is available, not unsaved drafts. Call workspace_get_selection/workspace_list_modules then workspace_read_module to read real content. Use workspace_propose_changes for ALL module edits; never edit .workspace manifests or SQLite through filesystem/shell tools. Pass runScope on every call and use the returned revision for expectedRevision. Pending means no change has been applied: tell the user review is required, and query workspace_get_proposal_result in the next turn. Do not report a pending proposal as successful. This is a fresh run scope; never reuse one from earlier turns.",context.module_id.as_deref().unwrap_or("none (workspace module list only)"))
    }
    pub fn module_tool_session_token(&self) -> Option<&str> {
        self.module_tool_lease
            .as_ref()
            .map(|lease| lease.token.as_str())
    }
    pub fn module_tool_run_scope(&self) -> Option<&str> {
        self.module_tool_lease
            .as_ref()?
            .run
            .as_ref()
            .map(|(scope, _)| scope.as_str())
    }
    fn module_provider(&self) -> String {
        self.state.agent.provider.clone().unwrap_or_else(|| {
            if self.state.agent.transport == "mock" {
                "mock"
            } else {
                "hermes"
            }
            .into()
        })
    }
    pub(super) fn begin_module_tool_session(&mut self, context: &ConnectorContext) {
        self.module_tool_lease = Some(ModuleToolLease {
            token: new_id(),
            provider: self.module_provider(),
            session: context.clone(),
            run: None,
        });
    }
    pub(super) fn begin_module_tool_run(
        &mut self,
        context: &ConnectorContext,
    ) -> Result<(), String> {
        let lease = self
            .module_tool_lease
            .as_mut()
            .ok_or("工作区工具会话尚未创建")?;
        if lease.session.runtime_session_id != context.runtime_session_id
            || lease.session.workspace_id != context.workspace_id
            || lease.session.root != context.root
            || lease.session.generation != context.generation
            || lease.session.permission_epoch != context.permission_epoch
        {
            return Err("工作区工具会话已失效".into());
        }
        lease.run = Some((new_id(), context.clone()));
        Ok(())
    }
    pub(super) fn invalidate_module_tools(&mut self, reason: &str) {
        self.module_tool_lease = None;
        for proposal in &mut self.state.module_proposals {
            if proposal.status == "pending" {
                proposal.status = "cancelled".into();
                proposal.summary = format!("提案已失效：{reason}");
                proposal.updated_at = now();
            }
        }
        self.state
            .approvals
            .retain(|approval| approval.kind != "module_changes");
    }
    fn validate_module_caller(
        &self,
        token: &str,
        scope: &str,
        result_only: bool,
    ) -> Result<ConnectorContext, String> {
        self.workspace_lock.ensure_valid()?;
        self.require_healthy_storage()?;
        self.require_agent_start()?;
        let lease = self
            .module_tool_lease
            .as_ref()
            .ok_or("工具连接已失效；请重新连接 Agent")?;
        if token != lease.token || lease.provider != self.module_provider() {
            return Err("工具连接身份无效".into());
        }
        let (expected_scope, context) = lease.run.as_ref().ok_or("当前没有已授权的工具运行")?;
        if scope != expected_scope {
            return Err("工具请求的运行授权已失效".into());
        }
        self.validate_connector_context(context)?;
        if !result_only
            && self
                .active
                .as_ref()
                .is_none_or(|run| run.connect_only || run.context != *context)
        {
            return Err("工具所属运行已经结束；仅可查询已存在的提案结果".into());
        }
        Ok(context.clone())
    }
    /// IPC request envelope correlation is Host-owned, distinct from a proposal's
    /// model-supplied idempotency key. Read errors are also auditable.
    pub fn handle_module_tool_request(
        &mut self,
        session_token: &str,
        run_scope: &str,
        tool: &str,
        args: Value,
        request_id: &str,
    ) -> Result<Value, String> {
        clean_key(request_id)?;
        self.validate_module_caller(session_token, run_scope, true)?;
        let mut result = self.handle_module_tool(session_token, run_scope, tool, args);
        if self.storage_fault.is_none() {
            self.audit(
                "workspace/tool_request",
                &format!(
                    "request={request_id}; tool={tool}; outcome={}",
                    if result.is_ok() { "ok" } else { "error" }
                ),
                &self.state.agent.id.clone(),
            )?;
            self.persist()?;
        }
        if let Ok(Value::Object(value)) = &mut result {
            value.insert("toolRequestId".into(), json!(request_id));
        }
        result
    }
    /// The only MCP dispatch entry. Canonical root, origin and provider identity
    /// come from Host state. Unknown fields (including root/origin) are rejected.
    pub fn handle_module_tool(
        &mut self,
        session_token: &str,
        run_scope: &str,
        tool: &str,
        mut args: Value,
    ) -> Result<Value, String> {
        if let Some(object) = args.as_object_mut() {
            if let Some(provided_scope) = object.remove("runScope") {
                if provided_scope.as_str() != Some(run_scope) {
                    return Err("工具参数运行授权与连接不一致".into());
                }
            }
        }
        let serialized = args.to_string();
        if self.redact_tool_credentials(&serialized) != serialized {
            return Err("工具参数不得将会话凭据或运行授权写入模块数据".into());
        }
        let name = tool
            .strip_prefix("workspace.")
            .or_else(|| tool.strip_prefix("workspace_"))
            .unwrap_or(tool);
        let context =
            self.validate_module_caller(session_token, run_scope, name == "get_proposal_result")?;
        if args.to_string().len() > MAX_TEXT * 2 {
            return Err("工具参数超过大小限制".into());
        }
        let result = match name {
            "list_modules" => {
                parse_empty(args)?;
                let modules = self.snapshot()?.modules;
                json!({"workspaceId":self.state.workspace_id,"workspaceRevision":self.workspace_module_revision()?,
                    "modules":modules.iter().map(|m| json!({"id":m.id,"type":m.module_type,"title":m.title,
                        "revision":m.module_revision,"allowedOperations":allowed_operations(m.module_type)})).collect::<Vec<_>>()})
            }
            "read_module" => {
                let input: ModuleIdInput = serde_json::from_value(args).map_err(json_error)?;
                let module = self.current_tool_module(&input.module_id)?;
                json!({"module":module,"revision":module.module_revision,"source":"saved",
                    "allowedOperations":allowed_operations(module.module_type),"metrics":self.dashboard_values(&module)})
            }
            "get_selection" => {
                parse_empty(args)?;
                let selected = context
                    .module_id
                    .as_ref()
                    .and_then(|id| self.state.modules.iter().find(|m| &m.id == id));
                json!({"moduleId":context.module_id,"source":"saved","draftIncluded":false,"selection":Value::Null,
                    "readable":selected.is_some_and(|m|m.module_type != ModuleType::Conversation),
                    "revision":selected.map(|m| self.current_tool_module(&m.id).ok().and_then(|m|m.module_revision))})
            }
            "propose_changes" => {
                self.workspace_lock
                    .check_external_writers_excluding(self.connector.managed_process_id())?;
                let input: ProposalInput = serde_json::from_value(args).map_err(json_error)?;
                return self.propose_module_change(context, input);
            }
            "get_proposal_result" => {
                let input: ProposalIdInput = serde_json::from_value(args).map_err(json_error)?;
                self.expire_module_proposal(&input.proposal_id)?;
                let record = self
                    .state
                    .module_proposals
                    .iter()
                    .find(|p| p.id == input.proposal_id)
                    .ok_or("提案不存在")?;
                if record.provider != self.module_provider() {
                    return Err("提案不属于当前 Provider".into());
                }
                return Ok(proposal_result(record));
            }
            _ => return Err("不支持的工作区工具".into()),
        };
        self.audit(
            "workspace/tool_read",
            &format!("{} request; run={}", name, context.run_id),
            &self.state.agent.id.clone(),
        )?;
        self.persist()?;
        Ok(result)
    }

    fn current_tool_module(&self, id: &str) -> Result<WorkspaceModule, String> {
        let mut module = self.module(id)?.clone();
        if module.module_type == ModuleType::Conversation {
            return Err("对话历史不属于非对话模块工具范围".into());
        }
        if module.module_type == ModuleType::Document {
            let path = module.file_path.as_deref().ok_or("文档缺少文件路径")?;
            module.content = self.read_document(path)?;
            module.revision = Some(content_revision(&module.content));
        }
        module.module_revision = Some(module_revision(&module));
        Ok(module)
    }
    fn workspace_module_revision(&self) -> Result<String, String> {
        let modules = self.snapshot()?.modules;
        Ok(content_revision(&json!({"workspaceId":self.state.workspace_id,"allowOverlap":self.state.allow_overlap,
            "modules":modules.iter().map(|m|json!({"id":m.id,"revision":m.module_revision})).collect::<Vec<_>>()}).to_string()))
    }
    fn dashboard_values(&self, module: &WorkspaceModule) -> Value {
        if module.module_type != ModuleType::Dashboard {
            return Value::Null;
        }
        let config = module.dashboard_config.clone().unwrap_or_default();
        let tasks = self
            .state
            .modules
            .iter()
            .filter(|m| {
                m.module_type == ModuleType::Planner
                    && (config.planner_ids.is_empty() || config.planner_ids.contains(&m.id))
            })
            .flat_map(|m| &m.tasks)
            .collect::<Vec<_>>();
        let all = json!({"tasks_done":tasks.iter().filter(|t|t.done).count(),"tasks_total":tasks.len(),
            "module_count":self.state.modules.len(),"document_count":self.state.modules.iter().filter(|m|m.module_type == ModuleType::Document).count()});
        let mut selected = serde_json::Map::new();
        for metric in config.metrics {
            let name = serde_json::to_value(metric)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            selected.insert(name.clone(), all[&name].clone());
        }
        Value::Object(selected)
    }
    fn validate_dashboard(&self, config: &DashboardConfig) -> Result<(), String> {
        if config.metrics.is_empty() || config.metrics.len() > 4 {
            return Err("看板需选择 1–4 个已有指标".into());
        }
        let names = config
            .metrics
            .iter()
            .map(|m| serde_json::to_string(m).unwrap())
            .collect::<HashSet<_>>();
        if names.len() != config.metrics.len() {
            return Err("看板指标不能重复".into());
        }
        if let Some(title) = &config.title {
            clean_title(title)?;
        }
        if config.planner_ids.len() > MAX_MODULES {
            return Err("看板筛选数量超限".into());
        }
        let mut ids = HashSet::new();
        for id in &config.planner_ids {
            if !ids.insert(id) || self.module(id)?.module_type != ModuleType::Planner {
                return Err("看板只能引用当前工作区的规划模块，且不能重复".into());
            }
        }
        Ok(())
    }
    fn validate_tool_layout(&self, target: &str, layout: &Layout) -> Result<(), String> {
        validate_layout(layout)?;
        if !self.state.allow_overlap
            && self
                .state
                .modules
                .iter()
                .any(|m| m.id != target && overlaps(layout, &m.layout))
        {
            return Err("布局与现有模块碰撞；请保留用户布局或提出空闲位置".into());
        }
        Ok(())
    }
    fn prepare_module_change(
        &self,
        input: &ProposalInput,
    ) -> Result<(Option<WorkspaceModule>, WorkspaceModule), String> {
        if let ModuleChange::Create {
            module_type,
            title,
            content,
            tasks,
            dashboard,
        } = &input.change
        {
            if input.expected_revision != self.workspace_module_revision()? {
                return Err("workspace revision 冲突：模块列表或布局已改变，请重新读取".into());
            }
            if *module_type == ModuleType::Conversation {
                return Err("模块工具不能创建对话模块".into());
            }
            if self.state.modules.len() >= MAX_MODULES {
                return Err("模块数量已达上限".into());
            }
            if content.is_some() && *module_type != ModuleType::Document
                || tasks.is_some() && *module_type != ModuleType::Planner
                || dashboard.is_some() && *module_type != ModuleType::Dashboard
            {
                return Err("创建内容与模块类型不匹配".into());
            }
            let id = new_id();
            let title = clean_title(title)?;
            let text = content.clone().unwrap_or_default();
            validate_text(&text, MAX_TEXT)?;
            let layout = Layout {
                x: 0,
                y: self
                    .state
                    .modules
                    .iter()
                    .map(|m| m.layout.y + m.layout.h)
                    .max()
                    .unwrap_or(0),
                w: if *module_type == ModuleType::Dashboard {
                    24
                } else {
                    12
                },
                h: if *module_type == ModuleType::Dashboard {
                    34
                } else {
                    43
                },
            };
            self.validate_tool_layout(&id, &layout)?;
            let config = if *module_type == ModuleType::Dashboard {
                Some(dashboard.clone().unwrap_or_default())
            } else {
                None
            };
            if let Some(config) = &config {
                self.validate_dashboard(config)?;
            }
            let mut after = WorkspaceModule {
                id: id.clone(),
                module_type: *module_type,
                title,
                status: ModuleStatus::Attention,
                layout,
                tasks: task_inputs(tasks.as_deref().unwrap_or(&[]))?,
                content: text.clone(),
                file_path: if *module_type == ModuleType::Document {
                    Some(format!("notes/{id}.md"))
                } else {
                    None
                },
                revision: if *module_type == ModuleType::Document {
                    Some(content_revision(&text))
                } else {
                    None
                },
                module_revision: None,
                dashboard_config: config,
            };
            after.module_revision = Some(module_revision(&after));
            return Ok((None, after));
        }
        let before = self.current_tool_module(input.change.target().ok_or("提案缺少模块目标")?)?;
        if before.module_revision.as_deref() != Some(&input.expected_revision) {
            return Err("module revision 冲突：用户或外部编辑已改变模块，请重新读取".into());
        }
        let mut after = before.clone();
        match &input.change {
            ModuleChange::Document { content, .. } => {
                if after.module_type != ModuleType::Document {
                    return Err("目标不是文档模块".into());
                }
                validate_text(content, MAX_TEXT)?;
                after.content = content.clone();
                after.revision = Some(content_revision(content));
            }
            ModuleChange::Planner { tasks, .. } => {
                if after.module_type != ModuleType::Planner {
                    return Err("目标不是规划模块".into());
                }
                after.tasks = task_inputs(tasks)?;
            }
            ModuleChange::Dashboard { config, .. } => {
                if after.module_type != ModuleType::Dashboard {
                    return Err("目标不是看板模块".into());
                }
                self.validate_dashboard(config)?;
                after.dashboard_config = Some(config.clone());
            }
            ModuleChange::Metadata { title, layout, .. } => {
                if title.is_none() && layout.is_none() {
                    return Err("公共属性修改不能为空".into());
                }
                if let Some(title) = title {
                    after.title = clean_title(title)?;
                }
                if let Some(layout) = layout {
                    self.validate_tool_layout(&after.id, layout)?;
                    after.layout = layout.clone();
                }
            }
            ModuleChange::Create { .. } => unreachable!(),
        }
        after.status = ModuleStatus::Attention;
        after.module_revision = Some(module_revision(&after));
        validate_module(&after)?;
        Ok((Some(before), after))
    }
    fn propose_module_change(
        &mut self,
        context: ConnectorContext,
        input: ProposalInput,
    ) -> Result<Value, String> {
        clean_key(&input.request_id)?;
        clean_key(&input.idempotency_key)?;
        let fingerprint = content_revision(
            &json!({"expectedRevision":input.expected_revision,"change":input.change}).to_string(),
        );
        if let Some(record) = self
            .state
            .module_proposals
            .iter()
            .find(|p| p.idempotency_key == input.idempotency_key)
        {
            if record.fingerprint != fingerprint || record.provider != self.module_provider() {
                return Err("幂等键已用于其他操作；不能重用".into());
            }
            return Ok(proposal_result(record));
        }
        if self.state.module_proposals.len() >= MAX_PROPOSALS {
            return Err("模块工具操作记录已达本轮上限".into());
        }
        let (before, after) = self.prepare_module_change(&input)?;
        if self
            .state
            .approvals
            .iter()
            .any(|a| a.module_id.as_deref() == Some(&after.id))
        {
            return Err("此模块已有待处理提案".into());
        }
        let id = new_id();
        let title = format!(
            "{}：{}",
            if before.is_some() {
                "修改模块"
            } else {
                "创建模块"
            },
            after.title
        );
        let record = ModuleProposal {
            id: id.clone(),
            request_id: input.request_id,
            idempotency_key: input.idempotency_key,
            fingerprint,
            provider: self.module_provider(),
            context: context.clone(),
            status: "pending".into(),
            title: title.clone(),
            module_id: after.id.clone(),
            expected_revision: input.expected_revision.clone(),
            result_revision: None,
            summary: "尚未应用；等待用户审阅".into(),
            created_at: now(),
            updated_at: now(),
            change: input.change.clone(),
            before: before.clone(),
            after: after.clone(),
        };
        self.state.approvals.push(Approval {
            id: id.clone(),
            kind: "module_changes".into(),
            title,
            description: if input.change.changes_layout() {
                "包含移动/缩放；需明确批准此布局提案。"
            } else {
                "Host 从当前已保存内容取得 before；批准时再次核对权限与 revision。"
            }
            .into(),
            module_id: before.as_ref().map(|m| m.id.clone()),
            module_type: Some(after.module_type),
            file_path: after.file_path.clone(),
            before: Some(serde_json::to_string_pretty(&before).map_err(json_error)?),
            after: Some(serde_json::to_string_pretty(&after).map_err(json_error)?),
            revision: Some(input.expected_revision),
            origin: "agent".into(),
            epoch: context.permission_epoch,
            options: Vec::new(),
            scope: None,
            changes: Some(serde_json::to_value(input.change).map_err(json_error)?),
            proposal_id: Some(id.clone()),
            context: Some(context.clone()),
        });
        self.state.module_proposals.push(record);
        self.state.run_status = RunStatus::WaitingApproval;
        self.audit(
            "workspace/proposal_pending",
            &format!(
                "proposal={id}; request={}; run={}",
                self.state.module_proposals.last().unwrap().request_id,
                context.run_id
            ),
            &self.state.agent.id.clone(),
        )?;
        // A proposal and its immutable identity are durable before automatic or
        // explicit approval. Full access does not infer permission to rearrange.
        self.persist()?;
        if self.effective_policy() == WorkspacePolicy::Full
            && !self
                .state
                .module_proposals
                .last()
                .unwrap()
                .change
                .changes_layout()
        {
            self.decide_module_proposal(&id, true)?;
            self.persist()?;
        }
        Ok(proposal_result(
            self.state
                .module_proposals
                .iter()
                .find(|p| p.id == id)
                .unwrap(),
        ))
    }
    fn expire_module_proposal(&mut self, id: &str) -> Result<(), String> {
        let expired = self
            .state
            .module_proposals
            .iter()
            .find(|p| p.id == id && p.status == "pending")
            .is_some_and(|p| {
                chrono::DateTime::parse_from_rfc3339(&p.created_at)
                    .map(|time| {
                        Utc::now().signed_duration_since(time).num_seconds() > PROPOSAL_TTL_SECONDS
                    })
                    .unwrap_or(true)
            });
        if expired {
            self.finish_module_proposal(
                id,
                "cancelled",
                "提案等待审批超时，请重新读取并提出修改",
                None,
            )?;
            self.persist()?;
        }
        Ok(())
    }
    fn finish_module_proposal(
        &mut self,
        id: &str,
        status: &str,
        summary: &str,
        revision: Option<String>,
    ) -> Result<(), String> {
        let record = self
            .state
            .module_proposals
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("模块提案记录不存在")?;
        record.status = status.into();
        record.summary = summary.into();
        record.result_revision = revision;
        record.updated_at = now();
        self.state.approvals.retain(|a| a.id != id);
        if self.state.approvals.is_empty() {
            self.state.run_status = if self.active.is_some() {
                RunStatus::Running
            } else {
                RunStatus::Completed
            };
        }
        self.audit(
            &format!("workspace/proposal_{status}"),
            &format!("proposal={id}; {summary}"),
            "host",
        )
    }
    pub(super) fn decide_module_proposal(&mut self, id: &str, allow: bool) -> Result<(), String> {
        self.workspace_lock.ensure_valid()?;
        self.require_healthy_storage()?;
        self.expire_module_proposal(id)?;
        let proposal = self
            .state
            .module_proposals
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or("模块提案不存在")?;
        if proposal.status != "pending" {
            return Err(format!("提案已是 {}，不能重复应用", proposal.status));
        }
        if !allow {
            return self.finish_module_proposal(id, "rejected", "用户拒绝；未应用任何修改", None);
        }
        let approval = self
            .state
            .approvals
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or("审批已失效")?;
        self.require_agent_write(&approval)?;
        let authorization: Result<(), String> = (|| {
            self.validate_connector_context(&proposal.context)?;
            let lease = self.module_tool_lease.as_ref().ok_or("工具连接已经结束")?;
            if lease.provider != proposal.provider
                || lease
                    .run
                    .as_ref()
                    .is_none_or(|(_, ctx)| ctx != &proposal.context)
                || approval.context.as_ref() != Some(&proposal.context)
                || approval.epoch != proposal.context.permission_epoch
            {
                return Err("提案原始运行身份已经失效".into());
            }
            Ok(())
        })();
        if let Err(error) = authorization {
            self.finish_module_proposal(id, "cancelled", &error, None)?;
            return Err(error);
        }
        let valid: Result<(), String> = (|| {
            if let Some(before) = &proposal.before {
                let current = self.current_tool_module(&before.id)?;
                if current.module_revision.as_deref() != Some(&proposal.expected_revision)
                    || current.file_path != before.file_path
                {
                    return Err(
                        "module revision 冲突：审批前模块已改变；请重新读取并提出修改".into(),
                    );
                }
            } else if proposal.expected_revision != self.workspace_module_revision()? {
                return Err("workspace revision 冲突：审批前模块列表已改变；请重新读取".into());
            }
            if proposal.change.changes_layout() || proposal.before.is_none() {
                self.validate_tool_layout(&proposal.after.id, &proposal.after.layout)?;
            }
            if let Some(config) = &proposal.after.dashboard_config {
                self.validate_dashboard(config)?;
            }
            validate_module(&proposal.after)
        })();
        if let Err(error) = valid {
            self.finish_module_proposal(id, "conflict", &error, None)?;
            return Err(error);
        }
        if proposal.before.is_none() {
            self.connection.execute("INSERT INTO create_operations(id,module_json,approval_id,status) VALUES(?1,?2,?3,'pending')",
                params![proposal.after.id,serde_json::to_string(&proposal.after).map_err(json_error)?,id]).map_err(db_error)?;
            self.storage_checkpoint("before_create_file")?;
            if let Some(path) = proposal.after.file_path.as_deref() {
                if let Err(error) = atomic_write(&self.root, path, &proposal.after.content, None) {
                    self.storage_fault = Some(error.clone());
                    return Err(error);
                }
            }
            self.storage_checkpoint("after_create_file")?;
            self.state.modules.push(proposal.after.clone());
        } else {
            if matches!(proposal.change, ModuleChange::Document { .. }) {
                let path = proposal
                    .after
                    .file_path
                    .as_deref()
                    .ok_or("文档目标无路径")?;
                let before_revision = proposal
                    .before
                    .as_ref()
                    .and_then(|m| m.revision.as_deref())
                    .ok_or("文档目标无 revision")?;
                self.connection.execute("INSERT INTO write_operations(id,module_id,path,before_revision,after_revision,status) VALUES(?1,?2,?3,?4,?5,'pending')",
                    params![id,proposal.module_id,path,before_revision,content_revision(&proposal.after.content)]).map_err(db_error)?;
                self.storage_checkpoint("before_file_write")?;
                if let Err(error) = atomic_write(
                    &self.root,
                    path,
                    &proposal.after.content,
                    Some(before_revision),
                ) {
                    self.storage_fault = Some(error.clone());
                    return Err(error);
                }
                self.storage_checkpoint("after_file_write")?;
            }
            *self.module_mut(&proposal.module_id)? = proposal.after.clone();
        }
        self.finish_module_proposal(
            id,
            "applied",
            "修改已通过 Host 校验并应用",
            proposal.after.module_revision.clone(),
        )
    }
    pub(super) fn recover_module_proposals(&mut self) -> Result<(), String> {
        let pending = self
            .state
            .module_proposals
            .iter()
            .filter(|p| p.status == "pending")
            .cloned()
            .collect::<Vec<_>>();
        for proposal in pending {
            // Only an existing durable operation may establish recovery success.
            // A fresh runtime never reauthorizes a pending previous-runtime action.
            let journalled: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM write_operations WHERE id=?1 AND status IN ('pending','applied')) OR EXISTS(SELECT 1 FROM create_operations WHERE approval_id=?1 AND status IN ('pending','applied'))",[&proposal.id],|r|r.get(0)).map_err(db_error)?;
            let applied = journalled
                && self
                    .current_tool_module(&proposal.module_id)
                    .is_ok_and(|m| module_revision(&m) == module_revision(&proposal.after));
            self.finish_module_proposal(
                &proposal.id,
                if applied { "applied" } else { "cancelled" },
                if applied {
                    "已从既有操作日志恢复完成结果；没有重复写入"
                } else {
                    "应用重启；旧运行授权不再有效，未重放提案"
                },
                if applied {
                    proposal.after.module_revision
                } else {
                    None
                },
            )?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModuleIdInput {
    module_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProposalIdInput {
    proposal_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}
fn parse_empty(value: Value) -> Result<(), String> {
    serde_json::from_value::<EmptyInput>(value)
        .map(|_| ())
        .map_err(json_error)
}
fn allowed_operations(kind: ModuleType) -> Vec<&'static str> {
    match kind {
        ModuleType::Document => vec!["document", "metadata"],
        ModuleType::Planner => vec!["planner", "metadata"],
        ModuleType::Dashboard => vec!["dashboard", "metadata"],
        ModuleType::Conversation => vec![],
    }
}
fn overlaps(a: &Layout, b: &Layout) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}
fn clean_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > 160 || key.chars().any(char::is_control) {
        Err("请求 ID 与幂等键需为 1–160 字节可见文本".into())
    } else {
        Ok(())
    }
}
fn task_inputs(inputs: &[TaskInput]) -> Result<Vec<Task>, String> {
    if inputs.len() > 500 {
        return Err("任务数量超限".into());
    }
    let mut ids = HashSet::new();
    inputs
        .iter()
        .map(|input| {
            let id = input.id.clone().unwrap_or_else(new_id);
            validate_id(&id)?;
            if !ids.insert(id.clone()) {
                return Err("任务 ID 不能重复".into());
            }
            validate_text(&input.time, 240)?;
            validate_text(&input.tag, 240)?;
            Ok(Task {
                id,
                title: clean_title(&input.title)?,
                done: input.done,
                time: input.time.clone(),
                tag: input.tag.clone(),
            })
        })
        .collect()
}
fn proposal_result(record: &ModuleProposal) -> Value {
    json!({"proposalId":record.id,"requestId":record.request_id,"status":record.status,
    "applied":record.status=="applied","moduleId":record.module_id,"revision":record.result_revision,"summary":record.summary,
    "nextAction":if record.status=="pending"{"Wait for user review; pending is not success. Query workspace_get_proposal_result in a subsequent turn."}else{"none"}})
}

/// MCP declarations are generated beside the Host parser so the thin sidecar
/// cannot silently acquire a broader write surface.
pub fn tool_definitions() -> Value {
    let scope = json!({"type":"string","description":"Host-issued run scope from the current user prompt. Never reuse an earlier turn's scope."});
    let id = json!({"type":"string","description":"Host module UUID returned by list/read."});
    let layout = json!({"type":"object","additionalProperties":false,"required":["x","y","w","h"],
        "properties":{"x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0,"maximum":50000},
        "w":{"type":"integer","minimum":6,"maximum":24},"h":{"type":"integer","minimum":24,"maximum":120}}});
    let task = json!({"type":"object","additionalProperties":false,"required":["title"],"properties":{
        "id":{"type":"string","description":"Preserve an existing task ID when editing. Omit for a new task; Host assigns a UUID."},
        "title":{"type":"string","minLength":1,"maxLength":120},"done":{"type":"boolean"},"time":{"type":"string"},"tag":{"type":"string"}}});
    let tasks = json!({"type":"array","maxItems":500,"items":task,"description":"The complete next task list. Include retained tasks with IDs; omission removes a task in the reviewable diff."});
    let dashboard = json!({"type":"object","additionalProperties":false,"required":["metrics"],"properties":{
        "metrics":{"type":"array","minItems":1,"maxItems":4,"uniqueItems":true,"items":{"type":"string","enum":["tasks_done","tasks_total","module_count","document_count"]}},
        "plannerIds":{"type":"array","items":{"type":"string"},"description":"Empty means all local planners; IDs must belong to this workspace."},"title":{"type":"string"}}});
    let change = json!({"oneOf":[
        {"type":"object","additionalProperties":false,"required":["type","moduleId","content"],"properties":{"type":{"const":"document"},"moduleId":id,"content":{"type":"string"}}},
        {"type":"object","additionalProperties":false,"required":["type","moduleId","tasks"],"properties":{"type":{"const":"planner"},"moduleId":id,"tasks":tasks}},
        {"type":"object","additionalProperties":false,"required":["type","moduleId","config"],"properties":{"type":{"const":"dashboard"},"moduleId":id,"config":dashboard}},
        {"type":"object","additionalProperties":false,"required":["type","moduleId"],"properties":{"type":{"const":"metadata"},"moduleId":id,"title":{"type":"string"},"layout":layout}},
        {"type":"object","additionalProperties":false,"required":["type","moduleType","title"],"properties":{"type":{"const":"create"},"moduleType":{"type":"string","enum":["document","planner","dashboard"]},"title":{"type":"string"},"content":{"type":"string"},"tasks":tasks,"dashboard":dashboard}}
    ]});
    json!([
        {"name":"workspace_list_modules","description":"List this workspace's module IDs/types/titles/revisions; returns workspaceRevision for creating a module. Does not dump document bodies.",
            "inputSchema":{"type":"object","additionalProperties":false,"required":["runScope"],"properties":{"runScope":scope}}},
        {"name":"workspace_read_module","description":"Read saved document, planner or dashboard content and Host revision. Does not read unsaved drafts or conversation history.",
            "inputSchema":{"type":"object","additionalProperties":false,"required":["runScope","moduleId"],"properties":{"runScope":scope,"moduleId":id}}},
        {"name":"workspace_get_selection","description":"Read the user's selected module identity; saved content only, no implicit draft/selection capture.",
            "inputSchema":{"type":"object","additionalProperties":false,"required":["runScope"],"properties":{"runScope":scope}}},
        {"name":"workspace_propose_changes","description":"Submit one typed change. expectedRevision must be the read_module revision, or list_modules workspaceRevision for create. Request and idempotency keys are nonempty unique strings. Host reads before, validates permissions and revisions, then asks user or applies authorized Full changes. Pending is not success; do not edit manifests or SQLite directly.",
            "inputSchema":{"type":"object","additionalProperties":false,"required":["runScope","requestId","idempotencyKey","expectedRevision","change"],"properties":{
            "runScope":scope,"requestId":{"type":"string","maxLength":160},"idempotencyKey":{"type":"string","maxLength":160},"expectedRevision":{"type":"string"},"change":change}}},
        {"name":"workspace_get_proposal_result","description":"Query pending/applied/rejected/conflict/cancelled and the actual result revision. May query an older proposal using the current turn's scope. Never interprets pending as successful.",
            "inputSchema":{"type":"object","additionalProperties":false,"required":["runScope","proposalId"],"properties":{"runScope":scope,"proposalId":{"type":"string"}}}}
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("atrio-module-tools-{}", new_id())))
        }
        fn open(&self) -> Kernel {
            Kernel::open(&self.0).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn policy(mode: WorkspacePolicy, epoch: u64) -> PolicySnapshot {
        PolicySnapshot {
            system: SystemPolicy::Workspace,
            local: mode,
            effective: mode,
            source: "workspace".into(),
            epoch,
            workspace_trusted: true,
            hermes_scope_accepted: false,
            agent_scope_accepted: false,
            scope_provider: None,
        }
    }
    fn start(kernel: &mut Kernel) -> (String, String) {
        kernel
            .dispatch(WorkspaceAction::Prompt {
                text: "工具边界测试".into(),
                module_id: None,
            })
            .unwrap();
        (
            kernel.module_tool_session_token().unwrap().into(),
            kernel.module_tool_run_scope().unwrap().into(),
        )
    }
    fn call(
        kernel: &mut Kernel,
        auth: &(String, String),
        tool: &str,
        args: Value,
    ) -> Result<Value, String> {
        kernel.handle_module_tool(&auth.0, &auth.1, tool, args)
    }
    fn doc(kernel: &Kernel) -> WorkspaceModule {
        kernel
            .snapshot()
            .unwrap()
            .modules
            .into_iter()
            .find(|m| m.module_type == ModuleType::Document)
            .unwrap()
    }
    fn propose(
        kernel: &mut Kernel,
        auth: &(String, String),
        key: &str,
        revision: &str,
        change: Value,
    ) -> Value {
        call(kernel,auth,"workspace_propose_changes",json!({"requestId":format!("request-{key}"),"idempotencyKey":key,"expectedRevision":revision,"change":change})).unwrap()
    }
    fn decide(
        kernel: &mut Kernel,
        result: &Value,
        allow: bool,
    ) -> Result<WorkspaceSnapshot, String> {
        kernel.dispatch(WorkspaceAction::DecideApproval {
            approval_id: result["proposalId"].as_str().unwrap().into(),
            allow,
        })
    }
    fn finished(kernel: &mut Kernel) {
        for _ in 0..1000 {
            if kernel.tick().unwrap().is_none() {
                return;
            }
        }
        panic!("Mock did not finish");
    }

    #[test]
    fn document_review_reads_host_before_and_applies_only_after_approval() {
        let f = Fixture::new();
        let mut k = f.open();
        let original = doc(&k);
        let auth = start(&mut k);
        let read = call(
            &mut k,
            &auth,
            "workspace_read_module",
            json!({"moduleId":original.id}),
        )
        .unwrap();
        let proposal = propose(
            &mut k,
            &auth,
            "doc-deny",
            read["revision"].as_str().unwrap(),
            json!({"type":"document","moduleId":original.id,"content":"# Different user objective\n"}),
        );
        assert_eq!(proposal["status"], "pending");
        assert_eq!(proposal["applied"], false);
        assert!(k.state.approvals[0]
            .before
            .as_ref()
            .unwrap()
            .contains("让想法有自己的位置"));
        assert_eq!(doc(&k).content, original.content);
        decide(&mut k, &proposal, false).unwrap();
        assert_eq!(doc(&k).content, original.content);
        let proposal = propose(
            &mut k,
            &auth,
            "doc-allow",
            read["revision"].as_str().unwrap(),
            json!({"type":"document","moduleId":original.id,"content":"# Approved exact content\n"}),
        );
        decide(&mut k, &proposal, true).unwrap();
        assert_eq!(
            fs::read_to_string(f.0.join(original.file_path.unwrap())).unwrap(),
            "# Approved exact content\n"
        );
        assert_eq!(
            call(
                &mut k,
                &auth,
                "workspace_get_proposal_result",
                json!({"proposalId":proposal["proposalId"]})
            )
            .unwrap()["status"],
            "applied"
        );
        drop(k);
        assert_eq!(doc(&f.open()).content, "# Approved exact content\n");
    }

    #[test]
    fn full_planner_creation_and_update_drive_filtered_dashboard_metrics() {
        let f = Fixture::new();
        let mut k = f.open();
        k.apply_policy(policy(WorkspacePolicy::Full, 1)).unwrap();
        let auth = start(&mut k);
        let workspace_revision = k.workspace_module_revision().unwrap();
        let created = propose(
            &mut k,
            &auth,
            "create-plan",
            &workspace_revision,
            json!({"type":"create","moduleType":"planner","title":"Train for autumn race","tasks":[{"title":"Run 5 km","time":"2026-10-01","tag":"fitness"},{"title":"Stretch","done":true}]}),
        );
        assert_eq!(created["status"], "applied");
        let id = created["moduleId"].as_str().unwrap();
        let planner = k.current_tool_module(id).unwrap();
        assert_eq!(planner.tasks.len(), 2);
        assert!(planner.tasks.iter().all(|t| Uuid::parse_str(&t.id).is_ok()));
        let edited = propose(
            &mut k,
            &auth,
            "update-plan",
            planner.module_revision.as_deref().unwrap(),
            json!({"type":"planner","moduleId":id,"tasks":[{"id":planner.tasks[0].id,"title":"Run 8 km","done":true,"time":"2026-10-02"}]}),
        );
        assert_eq!(edited["status"], "applied");
        let dashboard = k
            .state
            .modules
            .iter()
            .find(|m| m.module_type == ModuleType::Dashboard)
            .unwrap()
            .clone();
        let dash = k.current_tool_module(&dashboard.id).unwrap();
        propose(
            &mut k,
            &auth,
            "dashboard",
            dash.module_revision.as_deref().unwrap(),
            json!({"type":"dashboard","moduleId":dash.id,"config":{"metrics":["tasks_done","tasks_total"],"plannerIds":[id],"title":"Race preparation"}}),
        );
        let values = k.dashboard_values(k.module(&dash.id).unwrap());
        assert_eq!(values, json!({"tasks_done":1,"tasks_total":1}));
        k.dispatch(WorkspaceAction::ToggleTask {
            module_id: id.into(),
            task_id: planner.tasks[0].id.clone(),
        })
        .unwrap();
        assert_eq!(
            k.dashboard_values(k.module(&dash.id).unwrap())["tasks_done"],
            0
        );
        drop(k);
        let k = f.open();
        assert_eq!(k.module(id).unwrap().tasks[0].title, "Run 8 km");
        assert_eq!(
            k.module(&dash.id)
                .unwrap()
                .dashboard_config
                .as_ref()
                .unwrap()
                .title
                .as_deref(),
            Some("Race preparation")
        );
    }

    #[test]
    fn metadata_revision_changes_for_manual_title_layout_tasks_and_external_document() {
        let f = Fixture::new();
        let mut k = f.open();
        let original = doc(&k);
        let rev = original.module_revision.unwrap();
        k.dispatch(WorkspaceAction::RenameModule {
            module_id: original.id.clone(),
            title: "A different title".into(),
        })
        .unwrap();
        let changed = doc(&k);
        assert_ne!(changed.module_revision.as_deref(), Some(rev.as_str()));
        let next = changed.module_revision.unwrap();
        k.dispatch(WorkspaceAction::SetLayouts {
            layouts: vec![LayoutChange {
                id: original.id.clone(),
                layout: Layout {
                    x: 12,
                    y: 1000,
                    w: 12,
                    h: 47,
                },
            }],
        })
        .unwrap();
        assert_ne!(doc(&k).module_revision.as_deref(), Some(next.as_str()));
        let next = doc(&k).module_revision.unwrap();
        fs::write(
            f.0.join(original.file_path.unwrap()),
            "external saved version",
        )
        .unwrap();
        assert_ne!(doc(&k).module_revision.as_deref(), Some(next.as_str()));
    }

    #[test]
    fn revision_conflict_is_terminal_and_cannot_overwrite_user_changes() {
        let f = Fixture::new();
        let mut k = f.open();
        let original = doc(&k);
        let auth = start(&mut k);
        let proposal = propose(
            &mut k,
            &auth,
            "stale-doc",
            original.module_revision.as_deref().unwrap(),
            json!({"type":"document","moduleId":original.id,"content":"agent overwrite"}),
        );
        fs::write(f.0.join(original.file_path.unwrap()), "new user version").unwrap();
        assert!(decide(&mut k, &proposal, true)
            .unwrap_err()
            .contains("revision"));
        assert_eq!(doc(&k).content, "new user version");
        assert_eq!(
            call(
                &mut k,
                &auth,
                "workspace_get_proposal_result",
                json!({"proposalId":proposal["proposalId"]})
            )
            .unwrap()["status"],
            "conflict"
        );
        assert!(k.state.approvals.is_empty());
        assert!(decide(&mut k, &proposal, true).is_err());
    }

    #[test]
    fn idempotency_prevents_duplicate_creation_and_rejects_key_reuse() {
        let f = Fixture::new();
        let mut k = f.open();
        k.apply_policy(policy(WorkspacePolicy::Full, 1)).unwrap();
        let auth = start(&mut k);
        let revision = k.workspace_module_revision().unwrap();
        let args = json!({"requestId":"req","idempotencyKey":"stable-key","expectedRevision":revision,"change":{"type":"create","moduleType":"document","title":"Exactly once","content":"Persisted once"}});
        let first = call(&mut k, &auth, "workspace_propose_changes", args.clone()).unwrap();
        let second = call(&mut k, &auth, "workspace_propose_changes", args.clone()).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            k.state
                .modules
                .iter()
                .filter(|m| m.title == "Exactly once")
                .count(),
            1
        );
        let mut conflicting = args;
        conflicting["change"]["title"] = json!("Other operation");
        assert!(
            call(&mut k, &auth, "workspace_propose_changes", conflicting)
                .unwrap_err()
                .contains("幂等")
        );
    }

    #[test]
    fn cancel_rotates_connection_and_scope_old_requests_cannot_revive() {
        let f = Fixture::new();
        let mut k = f.open();
        let original = doc(&k);
        let auth = start(&mut k);
        let proposal = propose(
            &mut k,
            &auth,
            "cancelled",
            original.module_revision.as_deref().unwrap(),
            json!({"type":"document","moduleId":original.id,"content":"never"}),
        );
        k.dispatch(WorkspaceAction::Cancel).unwrap();
        assert!(call(&mut k, &auth, "workspace_list_modules", json!({})).is_err());
        let next = start(&mut k);
        assert_ne!(auth, next);
        assert!(k
            .handle_module_tool(&next.0, &auth.1, "workspace_list_modules", json!({}))
            .is_err());
        assert_eq!(
            call(
                &mut k,
                &next,
                "workspace_get_proposal_result",
                json!({"proposalId":proposal["proposalId"]})
            )
            .unwrap()["status"],
            "cancelled"
        );
        assert!(decide(&mut k, &proposal, true).is_err());
        assert_eq!(doc(&k).content, original.content);
    }

    #[test]
    fn stale_scope_is_not_substituted_by_latest_run_and_completed_run_is_read_only() {
        let f = Fixture::new();
        let mut k = f.open();
        let auth = start(&mut k);
        finished(&mut k);
        assert!(call(&mut k, &auth, "workspace_list_modules", json!({})).is_err());
        let next = start(&mut k);
        assert_eq!(auth.0, next.0);
        assert_ne!(auth.1, next.1);
        assert!(call(&mut k, &auth, "workspace_list_modules", json!({})).is_err());
        assert!(call(&mut k, &next, "workspace_list_modules", json!({})).is_ok());
    }

    #[test]
    fn agent_cannot_supply_origin_root_file_path_arbitrary_code_or_cross_workspace_ids() {
        let f = Fixture::new();
        let mut k = f.open();
        let auth = start(&mut k);
        let original = doc(&k);
        for args in [
            json!({"origin":"user"}),
            json!({"root":"/tmp"}),
            json!({"runScope":"wrong"}),
        ] {
            assert!(call(&mut k, &auth, "workspace_list_modules", args).is_err());
        }
        let mut args = json!({"requestId":"bad","idempotencyKey":"bad","expectedRevision":original.module_revision,"change":{"type":"document","moduleId":original.id,"content":"x","filePath":"../../escape"}});
        assert!(call(&mut k, &auth, "workspace_propose_changes", args.clone()).is_err());
        args["change"] = json!({"type":"sql","sql":"delete from modules"});
        assert!(call(&mut k, &auth, "workspace_propose_changes", args).is_err());
        assert!(call(
            &mut k,
            &auth,
            "workspace_read_module",
            json!({"moduleId":new_id()})
        )
        .is_err());
        let other = Fixture::new();
        let mut second = other.open();
        let second_auth = start(&mut second);
        assert!(second
            .handle_module_tool(&auth.0, &second_auth.1, "workspace_list_modules", json!({}))
            .is_err());
    }

    #[test]
    fn full_layout_requires_explicit_review_and_rechecks_collision() {
        let f = Fixture::new();
        let mut k = f.open();
        k.apply_policy(policy(WorkspacePolicy::Full, 1)).unwrap();
        let original = doc(&k);
        let auth = start(&mut k);
        let proposal = propose(
            &mut k,
            &auth,
            "move",
            original.module_revision.as_deref().unwrap(),
            json!({"type":"metadata","moduleId":original.id,"layout":{"x":12,"y":200,"w":12,"h":50}}),
        );
        assert_eq!(proposal["status"], "pending");
        assert_eq!(doc(&k).layout, original.layout);
        decide(&mut k, &proposal, true).unwrap();
        assert_eq!(doc(&k).layout.y, 200);
        let updated = doc(&k);
        let args = json!({"requestId":"bad-layout","idempotencyKey":"bad-layout","expectedRevision":updated.module_revision,"change":{"type":"metadata","moduleId":original.id,"layout":{"x":0,"y":0,"w":12,"h":47}}});
        assert!(call(&mut k, &auth, "workspace_propose_changes", args)
            .unwrap_err()
            .contains("碰撞"));
    }

    #[test]
    fn restricted_can_propose_but_never_apply_and_deny_invalidates_everything() {
        let f = Fixture::new();
        let mut k = f.open();
        k.apply_policy(policy(WorkspacePolicy::Restricted, 1))
            .unwrap();
        let original = doc(&k);
        let auth = start(&mut k);
        let proposal = propose(
            &mut k,
            &auth,
            "restricted",
            original.module_revision.as_deref().unwrap(),
            json!({"type":"document","moduleId":original.id,"content":"not permitted"}),
        );
        assert!(decide(&mut k, &proposal, true).is_err());
        assert_eq!(doc(&k).content, original.content);
        k.apply_policy(policy(WorkspacePolicy::Disabled, 2))
            .unwrap();
        assert!(call(&mut k, &auth, "workspace_list_modules", json!({})).is_err());
        assert_eq!(k.state.module_proposals[0].status, "cancelled");
        k.dispatch(WorkspaceAction::RenameModule {
            module_id: original.id,
            title: "User is still allowed".into(),
        })
        .unwrap();
        k.apply_policy(policy(WorkspacePolicy::Full, 3)).unwrap();
        assert!(decide(&mut k, &proposal, true).is_err());
    }

    #[test]
    fn pending_proposals_cancel_after_restart_but_journalled_document_write_recovers_once() {
        for checkpoint in [
            "before_file_write",
            "after_file_write",
            "after_pending_commit",
        ] {
            let f = Fixture::new();
            let mut k = f.open();
            let original = doc(&k);
            let auth = start(&mut k);
            let proposal = propose(
                &mut k,
                &auth,
                "interrupted",
                original.module_revision.as_deref().unwrap(),
                json!({"type":"document","moduleId":original.id,"content":"Recovered content"}),
            );
            k.failpoint = Some(checkpoint);
            assert!(decide(&mut k, &proposal, true).is_err());
            drop(k);
            let k = f.open();
            let status = k.state.module_proposals[0].status.as_str();
            if checkpoint == "before_file_write" {
                assert_eq!(status, "cancelled");
                assert_eq!(doc(&k).content, original.content);
            } else {
                assert_eq!(status, "applied");
                assert_eq!(doc(&k).content, "Recovered content");
            }
            assert!(k.state.approvals.is_empty());
            assert!(k.module_tool_session_token().is_none());
        }
    }

    #[test]
    fn create_and_metadata_recovery_use_existing_commit_journal() {
        for checkpoint in [
            "before_create_file",
            "after_create_file",
            "after_pending_commit",
        ] {
            let f = Fixture::new();
            let mut k = f.open();
            let auth = start(&mut k);
            let revision = k.workspace_module_revision().unwrap();
            let proposal = propose(
                &mut k,
                &auth,
                "create-recover",
                &revision,
                json!({"type":"create","moduleType":"document","title":"Recovery","content":"New document"}),
            );
            k.failpoint = Some(checkpoint);
            assert!(decide(&mut k, &proposal, true).is_err());
            drop(k);
            let k = f.open();
            if checkpoint == "before_create_file" {
                assert_eq!(k.state.module_proposals[0].status, "cancelled");
            } else {
                assert_eq!(k.state.module_proposals[0].status, "applied");
                assert_eq!(
                    k.state
                        .modules
                        .iter()
                        .filter(|m| m.title == "Recovery")
                        .count(),
                    1
                );
            }
        }
        let f = Fixture::new();
        let mut k = f.open();
        let original = doc(&k);
        let auth = start(&mut k);
        let proposal = propose(
            &mut k,
            &auth,
            "metadata-recover",
            original.module_revision.as_deref().unwrap(),
            json!({"type":"metadata","moduleId":original.id,"title":"Recovered title"}),
        );
        k.failpoint = Some("after_manifests");
        assert!(decide(&mut k, &proposal, true).is_err());
        drop(k);
        let k = f.open();
        assert_eq!(doc(&k).title, "Recovered title");
        assert_eq!(k.state.module_proposals[0].status, "applied");
    }

    #[test]
    fn approval_timeout_is_cancelled_and_never_approved() {
        let f = Fixture::new();
        let mut k = f.open();
        let original = doc(&k);
        let auth = start(&mut k);
        let proposal = propose(
            &mut k,
            &auth,
            "timeout",
            original.module_revision.as_deref().unwrap(),
            json!({"type":"document","moduleId":original.id,"content":"Late"}),
        );
        k.state.module_proposals[0].created_at = "2000-01-01T00:00:00Z".into();
        assert_eq!(
            call(
                &mut k,
                &auth,
                "workspace_get_proposal_result",
                json!({"proposalId":proposal["proposalId"]})
            )
            .unwrap()["status"],
            "cancelled"
        );
        assert!(decide(&mut k, &proposal, true).is_err());
    }

    #[test]
    fn ipc_request_ids_are_audited_and_pending_results_remain_truthful() {
        let f = Fixture::new();
        let mut k = f.open();
        let auth = start(&mut k);
        let result = k
            .handle_module_tool_request(
                &auth.0,
                &auth.1,
                "workspace_list_modules",
                json!({}),
                "ipc-unique-read",
            )
            .unwrap();
        assert_eq!(result["toolRequestId"], "ipc-unique-read");
        assert!(k
            .state
            .events
            .iter()
            .any(|e| e.kind == "workspace/tool_request" && e.message.contains("ipc-unique-read")));
        assert!(k
            .handle_module_tool_request(
                &auth.0,
                &auth.1,
                "workspace_read_module",
                json!({"moduleId":"not-a-module"}),
                "ipc-error-read"
            )
            .is_err());
        assert!(k
            .state
            .events
            .iter()
            .any(|e| e.message.contains("ipc-error-read") && e.message.contains("outcome=error")));
    }

    #[test]
    fn provider_terminal_events_cancel_pending_tools_without_user_cancel() {
        for terminal in [
            ConnectorEventKind::Cancelled,
            ConnectorEventKind::Disconnected {
                reason: "wire closed".into(),
            },
            ConnectorEventKind::Failed {
                code: "test".into(),
                message: "provider error".into(),
            },
        ] {
            let f = Fixture::new();
            let mut k = f.open();
            let original = doc(&k);
            let auth = start(&mut k);
            let proposal = propose(
                &mut k,
                &auth,
                "provider-ended",
                original.module_revision.as_deref().unwrap(),
                json!({"type":"document","moduleId":original.id,"content":"must not apply"}),
            );
            let context = k.active.as_ref().unwrap().context.clone();
            k.consume_connector_event(ConnectorEvent {
                context,
                seq: 1,
                kind: terminal,
            })
            .unwrap();
            assert_eq!(k.state.module_proposals[0].status, "cancelled");
            assert!(k.module_tool_session_token().is_none());
            assert!(decide(&mut k, &proposal, true).is_err());
            assert_eq!(doc(&k).content, original.content);
        }
    }

    #[test]
    fn module_badges_follow_remaining_approvals_and_preserve_applied_content() {
        let f = Fixture::new();
        let mut k = f.open();
        let first = doc(&k);
        k.dispatch(WorkspaceAction::CreateModule {
            module_type: ModuleType::Document,
            title: "Second review target".into(),
        })
        .unwrap();
        let second = k
            .current_tool_module(&k.state.modules.last().unwrap().id)
            .unwrap();
        let auth = start(&mut k);
        let one = propose(
            &mut k,
            &auth,
            "badge-one",
            first.module_revision.as_deref().unwrap(),
            json!({"type":"document","moduleId":first.id,"content":"Rejected content"}),
        );
        let two = propose(
            &mut k,
            &auth,
            "badge-two",
            second.module_revision.as_deref().unwrap(),
            json!({"type":"document","moduleId":second.id,"content":"Approved content"}),
        );
        finished(&mut k);
        assert_eq!(
            k.module(&first.id).unwrap().status,
            ModuleStatus::WaitingApproval
        );
        assert_eq!(
            k.module(&second.id).unwrap().status,
            ModuleStatus::WaitingApproval
        );
        decide(&mut k, &one, false).unwrap();
        assert_eq!(k.module(&first.id).unwrap().status, ModuleStatus::Idle);
        assert_eq!(
            k.module(&second.id).unwrap().status,
            ModuleStatus::WaitingApproval
        );
        assert!(k
            .state
            .modules
            .iter()
            .filter(|m| m.module_type == ModuleType::Conversation)
            .all(|m| m.status == ModuleStatus::WaitingApproval));
        decide(&mut k, &two, true).unwrap();
        assert_eq!(
            k.module(&second.id).unwrap().status,
            ModuleStatus::Attention
        );
        assert!(k.state.modules.iter().all(|m| !matches!(
            m.status,
            ModuleStatus::Running | ModuleStatus::WaitingApproval
        )));
        drop(k);
        let k = f.open();
        assert_eq!(
            k.module(&second.id).unwrap().status,
            ModuleStatus::Attention
        );
        assert_eq!(k.module(&second.id).unwrap().content, "Approved content");
    }

    #[test]
    fn module_badges_resume_running_after_live_review_then_clear_on_terminal_paths() {
        for terminal in ["cancel", "revoke", "disconnect", "shutdown"] {
            let f = Fixture::new();
            let mut k = f.open();
            k.apply_policy(policy(WorkspacePolicy::Ask, 1)).unwrap();
            let original = doc(&k);
            k.dispatch(WorkspaceAction::Prompt {
                text: "Keep working".into(),
                module_id: Some(original.id.clone()),
            })
            .unwrap();
            let auth = (
                k.module_tool_session_token().unwrap().to_owned(),
                k.module_tool_run_scope().unwrap().to_owned(),
            );
            let proposal = propose(
                &mut k,
                &auth,
                "live-review",
                original.module_revision.as_deref().unwrap(),
                json!({"type":"document","moduleId":original.id,"content":"unused"}),
            );
            assert_eq!(doc(&k).status, ModuleStatus::WaitingApproval);
            decide(&mut k, &proposal, false).unwrap();
            assert_eq!(doc(&k).status, ModuleStatus::Running);
            match terminal {
                "cancel" => {
                    k.dispatch(WorkspaceAction::Cancel).unwrap();
                }
                "revoke" => {
                    k.apply_policy(policy(WorkspacePolicy::Disabled, 2))
                        .unwrap();
                }
                "disconnect" => {
                    k.dispatch(WorkspaceAction::DisconnectAgent).unwrap();
                }
                _ => {
                    k.shutdown().unwrap();
                }
            }
            assert!(
                k.snapshot().unwrap().modules.iter().all(|m| !matches!(
                    m.status,
                    ModuleStatus::Running | ModuleStatus::WaitingApproval
                )),
                "{terminal}"
            );
            assert!(
                k.state.modules.iter().all(|m| !matches!(
                    m.status,
                    ModuleStatus::Running | ModuleStatus::WaitingApproval
                )),
                "persisted {terminal}"
            );
            drop(k);
            let k = f.open();
            assert!(
                k.state.modules.iter().all(|m| !matches!(
                    m.status,
                    ModuleStatus::Running | ModuleStatus::WaitingApproval
                )),
                "reopened {terminal}"
            );
        }
    }

    #[test]
    fn module_badges_normalize_legacy_transients_without_erasing_pending_user_review() {
        for overall in [
            RunStatus::Idle,
            RunStatus::Cancelled,
            RunStatus::Completed,
            RunStatus::WaitingApproval,
        ] {
            let f = Fixture::new();
            let k = f.open();
            let mut saved = k.state.clone();
            saved.run_status = overall;
            for (index, module) in saved.modules.iter_mut().enumerate() {
                module.status = if index % 2 == 0 {
                    ModuleStatus::Running
                } else {
                    ModuleStatus::WaitingApproval
                };
                fs::write(
                    f.0.join(format!(".workspace/modules/{}.json", module.id)),
                    serde_json::to_vec(module).unwrap(),
                )
                .unwrap();
            }
            k.connection
                .execute(
                    "UPDATE metadata SET value=?1 WHERE key='snapshot'",
                    [serde_json::to_string(&saved).unwrap()],
                )
                .unwrap();
            drop(k);
            let mut k = f.open();
            assert!(!matches!(
                k.state.run_status,
                RunStatus::Running | RunStatus::WaitingApproval
            ));
            assert!(
                k.state
                    .modules
                    .iter()
                    .all(|m| m.status == ModuleStatus::Idle),
                "{overall:?}"
            );
            let document = doc(&k);
            k.dispatch(WorkspaceAction::EditDocument {
                module_id: document.id.clone(),
                content: "Pending manual save".into(),
                revision: document.revision,
            })
            .unwrap();
            drop(k);
            let k = f.open();
            assert_eq!(
                k.module(&document.id).unwrap().status,
                ModuleStatus::WaitingApproval
            );
            assert_eq!(k.state.approvals.len(), 1);
        }
    }

    #[test]
    fn module_badges_do_not_treat_connect_only_as_a_document_run() {
        let f = Fixture::new();
        let mut k = f.open();
        let original = doc(&k);
        k.dispatch(WorkspaceAction::Prompt {
            text: "Synthetic connecting transition".into(),
            module_id: Some(original.id.clone()),
        })
        .unwrap();
        assert_eq!(doc(&k).status, ModuleStatus::Running);
        k.active.as_mut().unwrap().connect_only = true;
        assert!(k
            .snapshot()
            .unwrap()
            .modules
            .iter()
            .all(|m| m.status != ModuleStatus::Running));
        k.persist().unwrap();
        assert!(k
            .state
            .modules
            .iter()
            .all(|m| m.status != ModuleStatus::Running));
    }

    #[test]
    fn schemas_expose_only_five_typed_tools_and_manifest_never_persists_credentials() {
        let f = Fixture::new();
        let mut k = f.open();
        let auth = start(&mut k);
        k.configure_tool_bridge(
            PathBuf::from("/tmp/private.sock"),
            PathBuf::from("/apps/atrio"),
        );
        assert_eq!(tool_definitions().as_array().unwrap().len(), 5);
        assert_eq!(
            k.module_tool_mcp_servers()[0]["args"],
            json!(["--workspace-mcp"])
        );
        let db: String = k
            .connection
            .query_row("SELECT value FROM metadata WHERE key='snapshot'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let manifest = fs::read_to_string(f.0.join(".workspace/workspace.json")).unwrap();
        for text in [db, manifest] {
            assert!(!text.contains(&auth.0));
            assert!(!text.contains(&auth.1));
        }
    }
}
