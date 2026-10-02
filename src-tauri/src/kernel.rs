//! Local P0 workspace host. The webview submits intentions; only this module owns
//! file writes, approvals, revisions and the append-only audit log.
use crate::agent::native::NativeAcpConnector;
use crate::agent::providers::{probe_provider, NativeProvider};
use crate::agent::{
    mock::MockAgentConnector, AgentConnector, ConnectorContext, ConnectorEvent, ConnectorEventKind,
    ConnectorPrompt, ModuleContext,
};
use crate::locks::WorkspaceLock;
use crate::policy::{PolicySnapshot, SystemPolicy, WorkspacePolicy};
use crate::workspace_tools::{HostWorkspaceTools, ToolGateway, WorkspaceToolIntent};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

#[path = "module_tools.rs"]
pub mod module_tools;
use module_tools::{DashboardConfig, ModuleProposal, ModuleToolLease};

const MAX_TEXT: usize = 1_000_000;
const MAX_MODULES: usize = 100;
const RECENT_EVENTS: usize = 200;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModuleType {
    Conversation,
    Planner,
    Document,
    Dashboard,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModuleStatus {
    Idle,
    Running,
    WaitingApproval,
    Attention,
    Error,
    Offline,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Idle,
    Running,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Ask,
    Restricted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Layout {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub done: bool,
    pub time: String,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceModule {
    pub id: String,
    #[serde(rename = "type")]
    pub module_type: ModuleType,
    pub title: String,
    pub status: ModuleStatus,
    pub layout: Layout,
    pub tasks: Vec<Task>,
    pub content: String,
    pub file_path: Option<String>,
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_config: Option<DashboardConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDescriptor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub id: String,
    pub name: String,
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    pub probe_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub seq: i64,
    pub kind: String,
    pub message: String,
    pub timestamp: String,
    pub session_id: Option<String>,
    #[serde(default = "host_actor")]
    pub actor: String,
    #[serde(default)]
    pub context: Option<ConnectorContext>,
    #[serde(default)]
    pub connector_seq: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub module_id: Option<String>,
    pub module_type: Option<ModuleType>,
    pub file_path: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub revision: Option<String>,
    #[serde(default = "agent_origin")]
    pub origin: String,
    #[serde(default)]
    pub epoch: u64,
    #[serde(default)]
    pub options: Vec<crate::agent::PermissionOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ConnectorContext>,
}

fn agent_origin() -> String {
    "agent".into()
}
#[derive(Clone, Copy)]
enum ActionOrigin {
    User,
    Agent,
}
impl ActionOrigin {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionState {
    pub status: String,
    pub message: String,
    pub provider_session_id: Option<String>,
    pub capabilities: serde_json::Value,
    pub hermes_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default)]
    pub provider_version: Option<String>,
    #[serde(default)]
    pub probe: Option<serde_json::Value>,
}
impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            status: "disconnected".into(),
            message: "尚未连接；恢复本地历史不等于恢复 Agent 会话".into(),
            provider_session_id: None,
            capabilities: serde_json::Value::Null,
            hermes_version: None,
            provider: None,
            provider_version: None,
            probe: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub schema_version: u32,
    pub name: String,
    pub root_path: String,
    pub modules: Vec<WorkspaceModule>,
    pub messages: Vec<Message>,
    pub events: Vec<AgentEvent>,
    pub approvals: Vec<Approval>,
    pub run_status: RunStatus,
    pub session_id: Option<String>,
    pub permission_mode: PermissionMode,
    pub agent: AgentDescriptor,
    pub allow_overlap: bool,
    pub updated_at: String,
    #[serde(default)]
    pub workspace_generation: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub provider_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<PolicySnapshot>,
    #[serde(default)]
    pub connection: ConnectionState,
    #[serde(default)]
    pub module_proposals: Vec<ModuleProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceAction {
    CreateModule {
        module_type: ModuleType,
        title: String,
    },
    RenameModule {
        module_id: String,
        title: String,
    },
    CloseModule {
        module_id: String,
    },
    DuplicateModule {
        module_id: String,
    },
    SetLayouts {
        layouts: Vec<LayoutChange>,
    },
    ToggleTask {
        module_id: String,
        task_id: String,
    },
    AddTask {
        module_id: String,
        title: String,
    },
    EditDocument {
        module_id: String,
        content: String,
        revision: Option<String>,
    },
    DecideApproval {
        approval_id: String,
        allow: bool,
    },
    SetPermissionMode {
        mode: PermissionMode,
    },
    SetOverlap {
        allow: bool,
    },
    SaveAgent {
        agent: AgentDescriptor,
    },
    ProbeAgent,
    Prompt {
        text: String,
        module_id: Option<String>,
    },
    Cancel,
    SetWorkspacePolicy {
        mode: WorkspacePolicy,
    },
    SetSystemPolicy {
        mode: SystemPolicy,
    },
    ConfirmHermesScope {
        accepted: bool,
    },
    ConfirmAgentScope {
        accepted: bool,
    },
    ConnectAgent,
    DisconnectAgent,
    PermissionReply {
        approval_id: String,
        option_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutChange {
    pub id: String,
    pub layout: Layout,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceManifest {
    schema_version: u32,
    name: String,
    module_ids: Vec<String>,
    permission_mode: PermissionMode,
    allow_overlap: bool,
    agent: AgentDescriptor,
    #[serde(default)]
    workspace_id: String,
}

struct ActiveRun {
    pending_output: String,
    message_id: String,
    module_id: Option<String>,
    context: ConnectorContext,
    last_seq: u64,
    connect_only: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct PendingAudit {
    kind: String,
    message: String,
    actor: String,
    session_id: Option<String>,
    timestamp: String,
    #[serde(default)]
    context: Option<ConnectorContext>,
    #[serde(default)]
    connector_seq: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct PendingCommit {
    state: WorkspaceSnapshot,
    events: Vec<PendingAudit>,
    #[serde(default)]
    recovered_operation_states: Vec<(String, String)>,
}

pub struct Kernel {
    root: PathBuf,
    connection: Connection,
    state: WorkspaceSnapshot,
    active: Option<ActiveRun>,
    connector: Box<dyn AgentConnector>,
    connector_session_ready: bool,
    // Gate C integrates the effective Host policy here. Never supplied by an Agent.
    permission_epoch: u64,
    lifecycle_context: Option<ConnectorContext>,
    native_requests: std::collections::HashMap<String, (ConnectorContext, serde_json::Value)>,
    pending_audit: Vec<PendingAudit>,
    recovered_operation_states: Vec<(String, String)>,
    storage_fault: Option<String>,
    workspace_lock: WorkspaceLock,
    module_tool_lease: Option<ModuleToolLease>,
    tool_bridge_paths: Option<(PathBuf, PathBuf)>,
    #[cfg(test)]
    failpoint: Option<&'static str>,
}

impl Kernel {
    pub fn open(path: &Path) -> Result<Self, String> {
        #[cfg(windows)]
        crate::platform::windows::paths::validate_root_path(path)?;
        fs::create_dir_all(path).map_err(io_error)?;
        let root = fs::canonicalize(path).map_err(io_error)?;
        if !root.is_dir() {
            return Err("工作区必须是文件夹".into());
        }
        let workspace_lock = WorkspaceLock::acquire(&root)?;
        let workspace_dir = contained_path(&root, ".workspace")?;
        fs::create_dir_all(&workspace_dir).map_err(io_error)?;
        let database = contained_path(&root, ".workspace/workspace.sqlite3")?;
        let connection = Connection::open(database).map_err(db_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .map_err(db_error)?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS modules(id TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY, position INTEGER NOT NULL, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS approvals(id TEXT PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY, status TEXT NOT NULL, updated_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS provider_sessions(runtime_session_id TEXT PRIMARY KEY,
               provider_session_id TEXT NOT NULL, workspace_id TEXT NOT NULL, root TEXT NOT NULL,
               generation TEXT NOT NULL, permission_epoch INTEGER NOT NULL, agent_id TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL,
               message TEXT NOT NULL, timestamp TEXT NOT NULL, session_id TEXT, actor TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS write_operations(id TEXT PRIMARY KEY, module_id TEXT NOT NULL,
               path TEXT NOT NULL, before_revision TEXT NOT NULL, after_revision TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'pending');
             CREATE TABLE IF NOT EXISTS create_operations(id TEXT PRIMARY KEY, module_json TEXT NOT NULL,
               approval_id TEXT, status TEXT NOT NULL DEFAULT 'pending');
             CREATE TRIGGER IF NOT EXISTS events_no_update BEFORE UPDATE ON events
               BEGIN SELECT RAISE(ABORT, 'audit events are append-only'); END;
             CREATE TRIGGER IF NOT EXISTS events_no_delete BEFORE DELETE ON events
               BEGIN SELECT RAISE(ABORT, 'audit events are append-only'); END;
             PRAGMA user_version=1;"
        ).map_err(db_error)?;
        for (name, declaration) in [("context_json", "TEXT"), ("connector_seq", "INTEGER")] {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM pragma_table_info('events') WHERE name=?1)",
                    [name],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            if !exists {
                connection
                    .execute_batch(&format!(
                        "ALTER TABLE events ADD COLUMN {name} {declaration}"
                    ))
                    .map_err(db_error)?;
            }
        }
        let saved: Option<String> = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='snapshot'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        let pending: Option<String> = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='pending_commit'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        let pending = pending
            .map(|json| serde_json::from_str::<PendingCommit>(&json).map_err(json_error))
            .transpose()?;
        let recovering = pending.is_some();
        let (mut state, pending_audit, recovered_operation_states) = if let Some(pending) = pending
        {
            (
                pending.state,
                pending.events,
                pending.recovered_operation_states,
            )
        } else {
            (
                match saved {
                    Some(json) => {
                        serde_json::from_str::<WorkspaceSnapshot>(&json).map_err(json_error)?
                    }
                    None => default_state(&root),
                },
                Vec::new(),
                Vec::new(),
            )
        };
        if state.schema_version != 1 {
            return Err("工作区 schema 版本不受支持".into());
        }
        // Keep the recorded root until legacy absolute Agent directories are rebased.
        // The old directory need not exist after a workspace has been moved.
        let previous_root = PathBuf::from(&state.root_path);
        state.root_path = root.to_string_lossy().into_owned();
        state.workspace_generation = new_id();
        if state.workspace_id.is_empty() {
            state.workspace_id = new_id();
        }
        state.events = load_events(&connection)?;
        state.connection = ConnectionState::default();
        state.approvals.retain(|a| a.kind != "agent_permission");
        let mut kernel = Self {
            root,
            connection,
            state,
            active: None,
            connector: Box::new(MockAgentConnector::default()),
            connector_session_ready: false,
            permission_epoch: 0,
            lifecycle_context: None,
            native_requests: Default::default(),
            pending_audit,
            recovered_operation_states,
            storage_fault: None,
            workspace_lock,
            module_tool_lease: None,
            tool_bridge_paths: None,
            #[cfg(test)]
            failpoint: None,
        };
        let manifest_path = contained_path(&kernel.root, ".workspace/workspace.json")?;
        if recovering {
            // A durable pending snapshot precedes the individual manifest writes.
            // Finish that exact snapshot; partially-written JSON is never authority.
            for module in &kernel.state.modules {
                validate_module(module)?;
            }
        } else if manifest_path.exists() {
            let manifest: WorkspaceManifest = read_json(&manifest_path)?;
            if manifest.schema_version != 1 || manifest.module_ids.len() > MAX_MODULES {
                return Err("工作区 manifest 不受支持或模块数量超限".into());
            }
            let mut modules = Vec::new();
            let mut ids = HashSet::new();
            for id in manifest.module_ids {
                validate_id(&id)?;
                if !ids.insert(id.clone()) {
                    return Err("Manifest 存在重复模块 ID".into());
                }
                let path = contained_path(&kernel.root, &format!(".workspace/modules/{id}.json"))?;
                let mut module: WorkspaceModule = read_json(&path)?;
                if module.id != id {
                    return Err("模块 manifest ID 不一致".into());
                }
                validate_module(&module)?;
                kernel.refresh_document(&mut module)?;
                modules.push(module);
            }
            kernel.state.modules = modules;
            kernel.state.name = manifest.name;
            kernel.state.permission_mode = manifest.permission_mode;
            kernel.state.allow_overlap = manifest.allow_overlap;
            kernel.state.agent = manifest.agent;
            if !manifest.workspace_id.is_empty() {
                kernel.state.workspace_id = manifest.workspace_id;
            }
        } else if kernel.state.modules.is_empty()
            && kernel
                .connection
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM create_operations WHERE status='pending'",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_error)?
                == 0
        {
            kernel.insert_module(ModuleType::Conversation, "Agent 对话".into(), None)?;
            kernel.insert_module(
                ModuleType::Document,
                "工作区说明".into(),
                Some(initial_document().into()),
            )?;
            kernel.insert_module(ModuleType::Dashboard, "工作区概览".into(), None)?;
            kernel.audit("workspace/created", "已创建本地工作区与默认模块", "host")?;
        } else {
            // The SQLite snapshot can restore a missing index; Markdown remains authoritative.
            let mut modules = kernel.state.modules.clone();
            for module in &mut modules {
                validate_module(module)?;
                kernel.refresh_document(module)?;
            }
            kernel.state.modules = modules;
        }
        kernel.state.agent = normalize_agent(
            &kernel.root,
            Some(&previous_root),
            kernel.state.agent.clone(),
        )?;
        kernel.recover_write_operations()?;
        kernel.recover_create_operations()?;
        kernel.recover_module_proposals()?;
        if kernel.state.run_status == RunStatus::Running
            || kernel.state.run_status == RunStatus::WaitingApproval
                && kernel.state.approvals.is_empty()
        {
            kernel.state.run_status = RunStatus::Cancelled;
            kernel.audit(
                "cancelled",
                "检测到上次退出时未完成或失效的任务，已取消；Agent 不支持跨进程续跑",
                "host",
            )?;
        }
        if !kernel.state.approvals.is_empty() {
            kernel.state.run_status = RunStatus::WaitingApproval;
        }
        kernel.audit(
            "workspace/opened",
            "已恢复模块、文档、布局与最近事件",
            "host",
        )?;
        kernel.persist()?;
        Ok(kernel)
    }

    pub fn snapshot(&self) -> Result<WorkspaceSnapshot, String> {
        let mut snapshot = self.state.clone();
        // No watcher is claimed in P0. Every requested snapshot reads current Markdown.
        for module in &mut snapshot.modules {
            self.refresh_document(module)?;
            module.module_revision = Some(module_tools::module_revision(module));
        }
        reconcile_module_activity(&mut snapshot, self.active.as_ref());
        Ok(snapshot)
    }

    pub fn apply_policy(&mut self, next: PolicySnapshot) -> Result<bool, String> {
        let changed = self
            .state
            .policy
            .as_ref()
            .map(|old| serde_json::to_value(old).ok())
            != Some(serde_json::to_value(&next).ok());
        // Rehydrated snapshots retain policy, but the in-memory connector epoch
        // must always be refreshed from the authoritative channel store.
        self.permission_epoch = next.epoch;
        if !changed {
            return Ok(false);
        }
        let invalidates = self
            .state
            .policy
            .as_ref()
            .is_some_and(|old| old.epoch != next.epoch || old.effective != next.effective);
        self.permission_epoch = next.epoch;
        if invalidates {
            // Stop accepting old requests before waiting on external process shutdown.
            self.state
                .approvals
                .retain(|approval| approval.origin == "user");
            self.native_requests.clear();
            let had_run = self.active.is_some();
            self.state.policy = Some(next.clone());
            self.stop_connector("有效权限改变；旧 Agent 授权和提案已失效")?;
            if had_run {
                self.state.run_status = RunStatus::Cancelled;
            }
            self.audit(
                "permission/invalidated",
                "有效 Agent 权限改变，旧任务已回收；不会自动复活或批量批准",
                "host",
            )?;
        } else if self.state.policy.is_none() {
            // Migrate pre-policy, Host-created proposals conservatively; no auto-apply.
            for approval in &mut self.state.approvals {
                approval.epoch = next.epoch;
            }
        }
        self.state.permission_mode = if next.local == WorkspacePolicy::Restricted {
            PermissionMode::Restricted
        } else {
            PermissionMode::Ask
        };
        self.state.policy = Some(next);
        self.audit(
            "permission/effective",
            "已重新核对工作区有效策略与来源",
            "host",
        )?;
        self.persist()?;
        Ok(true)
    }

    pub fn policy_identity(&self) -> (&Path, &str, bool) {
        (
            &self.root,
            &self.state.workspace_id,
            self.state.policy.is_none() && self.state.permission_mode == PermissionMode::Restricted,
        )
    }

    fn effective_policy(&self) -> WorkspacePolicy {
        self.state.policy.as_ref().map(|p| p.effective).unwrap_or(
            if self.state.permission_mode == PermissionMode::Restricted {
                WorkspacePolicy::Restricted
            } else {
                WorkspacePolicy::Ask
            },
        )
    }
    pub fn selected_provider_id(&self) -> &str {
        self.state
            .agent
            .provider
            .as_deref()
            .unwrap_or(if self.state.agent.transport == "mock" {
                "mock"
            } else {
                "hermes"
            })
    }
    fn provider_label(&self) -> &str {
        match self.selected_provider_id() {
            "claude_code" => "Claude Code",
            "codex" => "Codex",
            "mock" => "Mock",
            _ => "Hermes",
        }
    }
    fn require_agent_start(&self) -> Result<(), String> {
        if self.effective_policy() == WorkspacePolicy::Disabled {
            return Err("当前权限禁止 Agent 运行；用户仍可手动编辑和布局".into());
        }
        if self.state.connection.status == "stopping" {
            return Err("Agent 尚未确认停止，请先断开重试".into());
        }
        if self.state.agent.transport != "mock" {
            if self.effective_policy() == WorkspacePolicy::Restricted {
                return Err(format!(
                    "{} 的完整只读隔离尚未验证，此模式不可连接；cwd 不是沙箱",
                    self.provider_label()
                ));
            }
            if self.effective_policy() == WorkspacePolicy::Ask
                && !self.state.policy.as_ref().is_some_and(|p| {
                    p.agent_scope_accepted
                        && p.scope_provider.as_deref() == Some(self.selected_provider_id())
                        || self.selected_provider_id() == "hermes"
                            && p.scope_provider.is_none()
                            && p.hermes_scope_accepted
                })
            {
                return Err(format!("请先明确确认本工作区 {} 本机运行范围；仅实际 ACP 请求受审批，Agent 自有工具并非全部由 Host 托管",self.provider_label()));
            }
        }
        Ok(())
    }
    fn require_agent_write(&self, approval: &Approval) -> Result<(), String> {
        if approval.epoch != self.permission_epoch {
            return Err("审批所属权限代次已失效，请重新提出任务".into());
        }
        match self.effective_policy() {
            WorkspacePolicy::Disabled | WorkspacePolicy::Restricted => {
                Err("当前有效权限阻止 Agent 提议写入".into())
            }
            _ => Ok(()),
        }
    }
    fn stop_connector(&mut self, reason: &str) -> Result<(), String> {
        self.invalidate_module_tools(reason);
        if let Some(run) = self.active.take() {
            let _ = self.connector.cancel(&run.context);
        }
        self.state.connection.status = "stopping".into();
        self.state.connection.message = reason.into();
        if let Err(error) = self.connector.shutdown() {
            self.state.connection.message = format!("停止失败：{error}");
            self.state.run_status = RunStatus::Failed;
            return Err(self.state.connection.message.clone());
        }
        self.connector = Box::new(MockAgentConnector::default());
        self.connector_session_ready = false;
        self.lifecycle_context = None;
        self.native_requests.clear();
        self.state.connection.status = "disconnected".into();
        self.state.connection.message = format!("{reason}；本应用管理的进程已回收");
        self.state.connection.provider_session_id = None;
        if self.state.approvals.is_empty()
            && matches!(
                self.state.run_status,
                RunStatus::Running | RunStatus::WaitingApproval
            )
        {
            self.state.run_status = RunStatus::Cancelled;
        }
        reconcile_module_activity(&mut self.state, self.active.as_ref());
        Ok(())
    }
    pub fn shutdown(&mut self) -> Result<(), String> {
        let was_running = self.active.is_some();
        self.stop_connector("应用退出")?;
        if was_running {
            self.state.run_status = RunStatus::Cancelled;
            self.audit("cancelled", "退出时已取消并回收 Agent", "host")?;
        }
        self.persist()
    }
    fn probe_agent(&mut self) -> Result<(), String> {
        if self.state.agent.transport == "mock" {
            self.state.agent.probe_status = "ready".into();
            return self.audit("agent/probe", "Mock 已就绪", "host");
        }
        let provider = NativeProvider::from_descriptor(&self.state.agent)?;
        if self.state.agent.args != provider.args() {
            return Err(format!(
                "{} 需要已验证的结构化启动参数 {:?}",
                provider.label(),
                provider.args()
            ));
        }
        match probe_provider(provider.id(), Some(&self.state.agent.command)) {
            Ok(probe) => {
                self.state.connection.provider = Some(provider.id().into());
                self.state.connection.provider_version = Some(probe.version.clone());
                self.state.connection.hermes_version = if provider.id() == "hermes" {
                    Some(probe.version.clone())
                } else {
                    None
                };
                self.state.connection.probe =
                    Some(serde_json::to_value(&probe).map_err(json_error)?);
                let available =
                    probe.host_available && probe.adapter_available && probe.acp_available;
                self.state.agent.probe_status = if available {
                    "available"
                } else {
                    "not_installed"
                }
                .into();
                if self.state.connection.status != "connected" {
                    self.state.connection.status = if available {
                        "disconnected"
                    } else {
                        "not_installed"
                    }
                    .into();
                }
                self.state.connection.message = if available {
                    format!(
                        "{} 宿主/ACP入口检查通过；认证与工具需实际会话验证",
                        provider.label()
                    )
                } else {
                    format!(
                        "{} 宿主或ACP入口不可用；请核对已安装的命令路径",
                        provider.label()
                    )
                };
                self.audit(
                    "agent/probe",
                    &format!(
                        "{} host={}, adapter={}, acp={}; 认证尚需握手与请求验证",
                        provider.label(),
                        probe.host_available,
                        probe.adapter_available,
                        probe.acp_available
                    ),
                    "host",
                )?;
                if available {
                    Ok(())
                } else {
                    Err(self.state.connection.message.clone())
                }
            }
            Err(error) => {
                self.state.connection.status =
                    if error.contains("未找到") || error.contains("不存在") {
                        "not_installed"
                    } else {
                        "error"
                    }
                    .into();
                self.state.connection.message = error.clone();
                self.state.agent.probe_status = self.state.connection.status.clone();
                Err(error)
            }
        }
    }

    fn connect_agent(&mut self) -> Result<(), String> {
        self.require_no_run()?;
        self.require_agent_start()?;
        if self.state.agent.transport == "mock" {
            return self.probe_agent();
        }
        if self.state.connection.status == "connected" && self.connector_session_ready {
            return Ok(());
        }
        self.stop_connector("建立新的 Provider 会话")?;
        self.probe_agent()?;
        let context = ConnectorContext {
            workspace_id: self.state.workspace_id.clone(),
            root: self.state.root_path.clone(),
            generation: self.state.workspace_generation.clone(),
            permission_epoch: self.permission_epoch,
            runtime_session_id: new_id(),
            run_id: new_id(),
            module_id: None,
        };
        self.begin_module_tool_session(&context);
        self.connector = Box::new(NativeAcpConnector::with_mcp(
            self.state.agent.clone(),
            self.module_tool_mcp_servers(),
        ));
        self.state.session_id = Some(context.runtime_session_id.clone());
        self.state.provider_session_id = None;
        self.lifecycle_context = Some(context.clone());
        self.state.connection.status = "connecting".into();
        self.state.connection.message = "正在 initialize 与 session/new…".into();
        self.active = Some(ActiveRun {
            pending_output: String::new(),
            message_id: String::new(),
            module_id: None,
            context: context.clone(),
            last_seq: 0,
            connect_only: true,
        });
        self.connector.initialize(&context)?;
        self.connector.new_session(&context)?;
        self.audit(
            "agent/connecting",
            "正在启动已授权的原生 ACP Provider 进程",
            "host",
        )
    }
    fn reply_permission(&mut self, id: &str, option_id: Option<String>) -> Result<(), String> {
        let approval = self
            .state
            .approvals
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or("权限请求不存在或已失效")?;
        if approval.kind != "agent_permission" {
            return Err("此审批不是 Agent 权限请求".into());
        }
        self.require_agent_write(&approval)?;
        let (context, request_id) = self
            .native_requests
            .get(id)
            .cloned()
            .ok_or("Agent 请求已退出；不能重放历史权限请求")?;
        self.validate_connector_context(&context)?;
        let allowed = option_id.as_ref().is_some_and(|id| {
            approval
                .options
                .iter()
                .any(|option| &option.id == id && option.kind.starts_with("allow_"))
        });
        if let Some(option) = &option_id {
            if !approval.options.iter().any(|item| &item.id == option) {
                return Err("权限选项不属于此请求".into());
            }
        }
        self.connector
            .permission_reply(&context, &request_id, option_id.as_deref())?;
        self.native_requests.remove(id);
        self.state.approvals.retain(|a| a.id != id);
        self.state.run_status = RunStatus::Running;
        self.audit(
            if allowed {
                "permission/approved"
            } else {
                "permission/rejected"
            },
            &format!("已回复 Agent 权限请求 {id}；Host 不重复执行工具"),
            "user",
        )
    }

    pub fn dispatch(&mut self, action: WorkspaceAction) -> Result<WorkspaceSnapshot, String> {
        self.workspace_lock.ensure_valid()?;
        if !matches!(
            action,
            WorkspaceAction::Cancel | WorkspaceAction::DisconnectAgent
        ) {
            self.workspace_lock
                .check_external_writers_excluding(self.connector.managed_process_id())?;
        }
        self.require_healthy_storage()?;
        match self.apply(action) {
            Ok(()) => {
                self.persist()?;
                self.snapshot()
            }
            Err(error) => {
                if self.storage_fault.is_some() {
                    return Err(error);
                }
                self.audit("host/error", &error, "host")?;
                self.persist()?;
                Err(error)
            }
        }
    }

    pub fn tick(&mut self) -> Result<Option<WorkspaceSnapshot>, String> {
        self.workspace_lock.ensure_valid()?;
        self.require_healthy_storage()?;
        let events = match self.connector.poll_events() {
            Ok(events) => events,
            Err(error) => {
                if self.active.is_some() || self.state.agent.transport != "mock" {
                    let module_id = self.active.as_ref().and_then(|run| run.module_id.clone());
                    let stopped = self.stop_connector("连接线程退出");
                    if stopped.is_ok() {
                        self.state.connection.status = "error".into();
                    }
                    self.state.connection.message = error.clone();
                    self.state.run_status = RunStatus::Failed;
                    self.set_target_status(module_id.as_deref(), ModuleStatus::Error);
                    self.audit("failed", &error, &self.state.agent.id.clone())?;
                    self.connector_session_ready = false;
                    self.persist()?;
                    return Ok(Some(self.snapshot()?));
                }
                return Err(error);
            }
        };
        let mut changed = false;
        let mut events: VecDeque<_> = events.into();
        while let Some(event) = events.pop_front() {
            if !self.accept_connector_event(&event) {
                continue;
            }
            changed = true;
            let context = event.context.clone();
            let session_created = matches!(event.kind, ConnectorEventKind::SessionCreated { .. });
            if let Err(error) = self.consume_connector_event(event) {
                if self.storage_fault.is_some() {
                    return Err(error);
                }
                let _ = self.connector.cancel(&context);
                let _ = self.stop_connector("Host 已拒绝本轮 Agent 事件");
                self.state.run_status = RunStatus::Failed;
                self.set_target_status(context.module_id.as_deref(), ModuleStatus::Error);
                self.push_message(
                    "assistant",
                    &format!("操作未完成：{error}。你可以检查事件记录后重试。"),
                );
                self.audit("failed", &error, "host")?;
            } else if session_created {
                // Mock may now emit its first chunk; Native's nonblocking poll
                // may return empty while its worker starts the queued prompt.
                events.extend(self.connector.poll_events()?);
            }
        }
        if !changed {
            return Ok(None);
        }
        self.persist()?;
        Ok(Some(self.snapshot()?))
    }

    fn validate_connector_context(&self, context: &ConnectorContext) -> Result<(), String> {
        if context.workspace_id != self.state.workspace_id
            || context.root != self.state.root_path
            || context.generation != self.state.workspace_generation
            || context.permission_epoch != self.permission_epoch
            || self.state.session_id.as_deref() != Some(&context.runtime_session_id)
        {
            return Err("Agent 事件所属工作区、会话或权限已失效".into());
        }
        Ok(())
    }

    fn accept_connector_event(&mut self, event: &ConnectorEvent) -> bool {
        if self.validate_connector_context(&event.context).is_err() {
            return false;
        }
        let Some(run) = self.active.as_mut() else {
            return self.lifecycle_context.as_ref() == Some(&event.context)
                && matches!(event.kind, ConnectorEventKind::Disconnected { .. });
        };
        if run.context != event.context || event.seq <= run.last_seq {
            return false;
        }
        run.last_seq = event.seq;
        true
    }

    fn audit_connector(
        &mut self,
        event: &ConnectorEvent,
        kind: &str,
        message: &str,
    ) -> Result<(), String> {
        self.pending_audit.push(PendingAudit {
            kind: kind.into(),
            message: self.redact_tool_credentials(message),
            actor: self.state.agent.id.clone(),
            session_id: Some(event.context.runtime_session_id.clone()),
            timestamp: now(),
            context: Some(event.context.clone()),
            connector_seq: Some(event.seq),
        });
        Ok(())
    }

    fn consume_connector_event(&mut self, event: ConnectorEvent) -> Result<(), String> {
        match &event.kind {
            ConnectorEventKind::Initialized {
                protocol_version,
                capabilities,
            } => {
                self.state.connection.capabilities = capabilities.clone();
                self.audit_connector(
                    &event,
                    "initialize",
                    &format!(
                        "Agent protocolVersion {protocol_version} / capabilities: {capabilities}"
                    ),
                )?;
            }
            ConnectorEventKind::SessionCreated {
                provider_session_id,
            } => {
                self.state.provider_session_id = Some(provider_session_id.clone());
                self.audit_connector(
                    &event,
                    "session/new",
                    &format!("建立会话；providerSessionId={provider_session_id}"),
                )?;
                // Do not acknowledge to the worker until the provider/runtime
                // mapping and session audit are durable. Prompt is gated by ack.
                self.persist()?;
                self.connector
                    .confirm_session_persisted(&event.context, provider_session_id)?;
                self.connector_session_ready = true;
                self.state.connection.status = "connected".into();
                self.state.connection.message = if self.state.agent.transport == "mock" {
                    "本地 Mock"
                } else {
                    "原生 Agent 已握手；当前会话可继续多轮，本地历史不代表跨进程续接"
                }
                .into();
                self.state.connection.provider_session_id = Some(provider_session_id.clone());
                if let Some(probe) = self.state.connection.probe.as_mut() {
                    probe["handshakeStatus"] = "passed".into();
                    probe["sessionStatus"] = "passed".into();
                }
                if self.active.as_ref().is_some_and(|run| run.connect_only) {
                    self.active = None;
                    self.state.run_status = RunStatus::Idle;
                }
            }
            ConnectorEventKind::TextDelta { text } => {
                let text = self.redact_stream_delta(text, false);
                let run = self.active.as_ref().ok_or("Agent 运行已结束")?;
                let message = self
                    .state
                    .messages
                    .iter_mut()
                    .find(|message| message.id == run.message_id)
                    .ok_or("流式消息已丢失")?;
                if message.text.len().saturating_add(text.len()) > MAX_TEXT {
                    return Err("Agent 输出超过 1 MB 上限".into());
                }
                message.text.push_str(&text);
                if !text.is_empty() {
                    self.audit_connector(&event, "session/update", &text)?;
                }
            }
            ConnectorEventKind::ToolProposal { proposal } => {
                self.require_agent_start()?;
                ToolGateway::propose(self, &event.context, proposal.clone())?;
                if self.effective_policy() == WorkspacePolicy::Full {
                    let ids = self
                        .state
                        .approvals
                        .iter()
                        .filter(|a| {
                            a.origin == "agent"
                                && a.epoch == self.permission_epoch
                                && a.kind != "agent_permission"
                        })
                        .map(|a| a.id.clone())
                        .collect::<Vec<_>>();
                    for id in ids {
                        self.decide_approval(&id, true)?;
                    }
                }
                self.audit_connector(
                    &event,
                    "connector/tool_proposal",
                    "已向 Host 提交结构化提案，等待审批",
                )?;
            }
            ConnectorEventKind::ToolStatus {
                tool_call_id,
                title,
                status,
            } => {
                // An ACP notification only changes presentation/audit. It must
                // never trigger the Host tool a second time.
                self.audit_connector(
                    &event,
                    "tool/status",
                    &format!("{tool_call_id}: {title} ({status})"),
                )?;
            }
            ConnectorEventKind::Completed { stop_reason } => {
                let tail = self.redact_stream_delta("", true);
                if !tail.is_empty() {
                    if let Some(run) = &self.active {
                        if let Some(message) = self
                            .state
                            .messages
                            .iter_mut()
                            .find(|m| m.id == run.message_id)
                        {
                            message.text.push_str(&tail);
                        }
                    }
                    self.audit_connector(&event, "session/update", &tail)?;
                }
                self.active = None;
                if self.state.agent.transport != "mock" {
                    if let Some(probe) = self.state.connection.probe.as_mut() {
                        probe["authenticationStatus"] = "request_succeeded".into();
                    }
                }
                if self.state.approvals.is_empty() {
                    self.state.run_status = RunStatus::Completed;
                    self.set_target_status(
                        event.context.module_id.as_deref(),
                        ModuleStatus::Attention,
                    );
                    self.audit_connector(
                        &event,
                        "completed",
                        &format!("Agent 对话已完成：{stop_reason}"),
                    )?;
                } else {
                    self.state.run_status = RunStatus::WaitingApproval;
                    self.set_target_status(
                        event.context.module_id.as_deref(),
                        ModuleStatus::WaitingApproval,
                    );
                }
            }
            ConnectorEventKind::Failed { code, message } => {
                self.invalidate_module_tools("Agent failed");
                self.connector_session_ready = false;
                self.active = None;
                if self.state.agent.transport != "mock" {
                    let stopped = self.stop_connector("Agent 运行失败");
                    if stopped.is_ok() {
                        self.state.connection.status = if code == "auth_required" {
                            "not_authenticated"
                        } else if code == "not_installed" {
                            "not_installed"
                        } else {
                            "error"
                        }
                        .into();
                    }
                    self.state.connection.message = match stopped {
                        Ok(_) => format!("{code}: {message}"),
                        Err(stop) => format!("{code}: {message}；{stop}"),
                    };
                    self.state
                        .approvals
                        .retain(|a| a.kind != "agent_permission");
                }
                self.state.run_status = RunStatus::Failed;
                self.set_target_status(event.context.module_id.as_deref(), ModuleStatus::Error);
                self.audit_connector(&event, "failed", &format!("{code}：{message}"))?;
            }
            ConnectorEventKind::Cancelled => {
                self.invalidate_module_tools("Agent cancelled");
                self.connector_session_ready = false;
                self.active = None;
                self.state.run_status = RunStatus::Cancelled;
                self.set_target_status(event.context.module_id.as_deref(), ModuleStatus::Attention);
                self.audit_connector(&event, "cancelled", "Agent 已停止")?;
            }
            ConnectorEventKind::Disconnected { reason } => {
                self.invalidate_module_tools("Agent disconnected");
                self.active = None;
                self.connector_session_ready = false;
                self.state.connection.status = "disconnected".into();
                self.state.connection.message = reason.clone();
                self.state.run_status = RunStatus::Failed;
                self.set_target_status(event.context.module_id.as_deref(), ModuleStatus::Offline);
                self.audit_connector(&event, "disconnected", reason)?;
            }
            ConnectorEventKind::PermissionRequest {
                request_id,
                title,
                options,
            } => {
                self.require_agent_start()?;
                if self.effective_policy() == WorkspacePolicy::Full {
                    let allowed = options
                        .iter()
                        .find(|option| option.kind == "allow_once")
                        .map(|option| option.id.as_str());
                    self.connector
                        .permission_reply(&event.context, request_id, allowed)?;
                    self.audit_connector(
                        &event,
                        "permission/auto",
                        "完全访问策略：仅回复本次合法选项；Host 不重复执行工具",
                    )?;
                } else {
                    let id = new_id();
                    self.native_requests
                        .insert(id.clone(), (event.context.clone(), request_id.clone()));
                    self.state.approvals.push(Approval {
                        id,
                        kind: "agent_permission".into(),
                        title: self.redact_tool_credentials(title),
                        description: format!(
                            "{} 请求权限；此审批仅控制这一次 ACP 请求，不代表其全部自有工具被托管",
                            self.provider_label()
                        ),
                        module_id: event.context.module_id.clone(),
                        module_type: None,
                        file_path: None,
                        before: None,
                        after: None,
                        revision: None,
                        origin: "agent".into(),
                        epoch: self.permission_epoch,
                        options: options.clone(),
                        scope: Some(self.state.root_path.clone()),
                        changes: None,
                        proposal_id: None,
                        context: Some(event.context.clone()),
                    });
                    self.state.run_status = RunStatus::WaitingApproval;
                    self.audit_connector(&event, "permission/request", "等待本次 Agent 权限决定")?;
                }
            }
        }
        Ok(())
    }

    fn apply(&mut self, action: WorkspaceAction) -> Result<(), String> {
        match action {
            WorkspaceAction::CreateModule { module_type, title } => {
                if module_type == ModuleType::Document && self.state.policy.is_none() {
                    self.require_writes()?;
                }
                let id = self.insert_module(module_type, clean_title(&title)?, None)?;
                self.audit("module/created", &format!("用户创建模块 {id}"), "user")?;
            }
            WorkspaceAction::RenameModule { module_id, title } => {
                let title = clean_title(&title)?;
                self.module_mut(&module_id)?.title = title.clone();
                self.audit(
                    "module/renamed",
                    &format!("模块 {module_id} → {title}"),
                    "user",
                )?;
            }
            WorkspaceAction::CloseModule { module_id } => {
                self.module(&module_id)?;
                if self.active.as_ref().and_then(|r| r.module_id.as_ref()) == Some(&module_id)
                    || self
                        .state
                        .approvals
                        .iter()
                        .any(|a| a.module_id.as_ref() == Some(&module_id))
                {
                    return Err("此模块仍有关联任务或审批，请先取消或处理审批".into());
                }
                self.state.modules.retain(|m| m.id != module_id);
                self.audit(
                    "module/closed",
                    &format!("关闭模块 {module_id}；保留关联文件"),
                    "user",
                )?;
            }
            WorkspaceAction::DuplicateModule { module_id } => {
                let source = self.module(&module_id)?.clone();
                if source.module_type == ModuleType::Document && self.state.policy.is_none() {
                    self.require_writes()?;
                }
                let mut fresh = source.clone();
                self.refresh_document(&mut fresh)?;
                let id = self.insert_module(
                    source.module_type,
                    format!("{} 副本", source.title),
                    Some(fresh.content),
                )?;
                self.module_mut(&id)?.tasks = source
                    .tasks
                    .into_iter()
                    .map(|mut t| {
                        t.id = new_id();
                        t
                    })
                    .collect();
                self.module_mut(&id)?.dashboard_config = source.dashboard_config.clone();
                let copied = serde_json::to_string(self.module(&id)?).map_err(json_error)?;
                self.connection.execute("UPDATE create_operations SET module_json=?1 WHERE id=?2 AND status='pending'",params![copied,id]).map_err(db_error)?;
                self.audit(
                    "module/duplicated",
                    &format!("复制模块 {module_id} → {id}"),
                    "user",
                )?;
            }
            WorkspaceAction::SetLayouts { layouts } => {
                if layouts.len() > MAX_MODULES {
                    return Err("布局数量超限".into());
                }
                let mut ids = HashSet::new();
                for change in &layouts {
                    self.module(&change.id)?;
                    validate_layout(&change.layout)?;
                    if !ids.insert(change.id.clone()) {
                        return Err("布局存在重复模块 ID".into());
                    }
                }
                for change in layouts {
                    self.module_mut(&change.id)?.layout = change.layout;
                }
                self.audit("layout/updated", "已保存用户布局", "user")?;
            }
            WorkspaceAction::ToggleTask { module_id, task_id } => {
                let module = self.module_mut(&module_id)?;
                if module.module_type != ModuleType::Planner {
                    return Err("目标不是规划模块".into());
                }
                let task = module
                    .tasks
                    .iter_mut()
                    .find(|t| t.id == task_id)
                    .ok_or("任务不存在")?;
                task.done = !task.done;
                self.audit("task/updated", &format!("已更新任务 {task_id}"), "user")?;
            }
            WorkspaceAction::AddTask { module_id, title } => {
                let title = clean_title(&title)?;
                let module = self.module_mut(&module_id)?;
                if module.module_type != ModuleType::Planner {
                    return Err("目标不是规划模块".into());
                }
                if module.tasks.len() >= 500 {
                    return Err("任务数量超限".into());
                }
                module.tasks.push(Task {
                    id: new_id(),
                    title,
                    done: false,
                    time: "待安排".into(),
                    tag: "自定义".into(),
                });
                self.audit("task/created", "已添加规划任务", "user")?;
            }
            WorkspaceAction::EditDocument {
                module_id,
                content,
                revision,
            } => {
                validate_text(&content, MAX_TEXT)?;
                let module = self.module(&module_id)?.clone();
                if module.module_type != ModuleType::Document {
                    return Err("目标不是文档模块".into());
                }
                let path = module.file_path.as_deref().ok_or("文档没有文件路径")?;
                let before = self.read_document(path)?;
                if revision.as_deref() != Some(&content_revision(&before)) {
                    return Err("文档已被外部修改，请刷新后重新生成修改".into());
                }
                if before == content {
                    return Ok(());
                }
                self.stage_write(&module_id, before, content, "用户提交文档修改", "user")?;
            }
            WorkspaceAction::DecideApproval { approval_id, allow } => {
                self.decide_approval(&approval_id, allow)?
            }
            WorkspaceAction::SetPermissionMode { mode } => {
                self.state.permission_mode = mode;
                self.audit(
                    "permission/mode",
                    if mode == PermissionMode::Restricted {
                        "切换至受限模式：仅生成 diff，禁止应用写入"
                    } else {
                        "切换至逐次审批模式"
                    },
                    "user",
                )?;
            }
            WorkspaceAction::SetOverlap { allow } => {
                self.state.allow_overlap = allow;
                self.audit(
                    "layout/overlap",
                    if allow {
                        "允许模块重叠"
                    } else {
                        "关闭模块重叠"
                    },
                    "user",
                )?;
            }
            WorkspaceAction::SaveAgent { mut agent } => {
                self.require_no_run()?;
                agent = normalize_agent(&self.root, None, agent)?;
                let same = agent.transport == self.state.agent.transport
                    && agent.command == self.state.agent.command
                    && agent.args == self.state.agent.args
                    && agent.cwd == self.state.agent.cwd
                    && agent.env == self.state.agent.env
                    && agent.provider == self.state.agent.provider;
                if !same {
                    self.stop_connector("Agent 配置已改变")?;
                }
                agent.probe_status = if agent.transport == "mock" {
                    "ready"
                } else {
                    "disconnected"
                }
                .into();
                self.state.agent = agent;
                if !same {
                    self.state.session_id = None;
                    self.state.provider_session_id = None;
                    self.state.connection = ConnectionState::default();
                }
                self.audit(
                    "agent/saved",
                    "已保存 Agent 描述；未自动启动或回退到 Mock",
                    "user",
                )?;
            }
            WorkspaceAction::ProbeAgent => self.probe_agent()?,
            WorkspaceAction::ConnectAgent => self.connect_agent()?,
            WorkspaceAction::DisconnectAgent => {
                self.stop_connector("用户断开连接")?;
                self.state.approvals.retain(|a| a.origin == "user");
                if self.state.run_status == RunStatus::Running
                    || self.state.run_status == RunStatus::WaitingApproval
                {
                    self.state.run_status = RunStatus::Cancelled;
                }
                self.audit("agent/disconnected", "用户已断开，关联进程已回收", "user")?;
            }
            WorkspaceAction::PermissionReply {
                approval_id,
                option_id,
            } => self.reply_permission(&approval_id, option_id)?,
            WorkspaceAction::SetWorkspacePolicy { .. }
            | WorkspaceAction::SetSystemPolicy { .. }
            | WorkspaceAction::ConfirmHermesScope { .. }
            | WorkspaceAction::ConfirmAgentScope { .. } => {
                return Err("权限配置必须经应用级可信策略入口处理".into())
            }
            WorkspaceAction::Prompt { text, module_id } => self.start_run(text, module_id)?,
            WorkspaceAction::Cancel => {
                if self.active.is_none()
                    && self.state.approvals.is_empty()
                    && self.state.run_status != RunStatus::Running
                {
                    return Err("当前没有可取消的任务".into());
                }
                self.stop_connector("用户取消任务")?;
                let count = self.state.approvals.len();
                self.state.approvals.clear();
                self.state.run_status = RunStatus::Cancelled;
                for module in &mut self.state.modules {
                    if matches!(
                        module.status,
                        ModuleStatus::Running | ModuleStatus::WaitingApproval
                    ) {
                        module.status = ModuleStatus::Idle;
                    }
                }
                self.audit(
                    "session/cancel",
                    &format!("用户取消任务，并撤销 {count} 个待处理审批"),
                    "user",
                )?;
                self.audit(
                    "cancelled",
                    "Agent 已停止，后续流式输出和操作已失效",
                    "host",
                )?;
            }
        }
        Ok(())
    }

    fn start_run(&mut self, text: String, module_id: Option<String>) -> Result<(), String> {
        self.require_no_run()?;
        self.require_agent_start()?;
        validate_text(&text, 16_000)?;
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("请输入任务内容".into());
        }
        if self.state.agent.transport != "mock"
            && (!self.connector_session_ready || self.state.connection.status != "connected")
        {
            return Err(format!(
                "{} 尚未连接，请先连接并完成运行范围确认",
                self.provider_label()
            ));
        }
        if let Some(id) = &module_id {
            self.module(id)?;
        }
        let modules = if self.state.agent.transport == "mock" {
            self.module_context()?
        } else {
            Vec::new()
        };
        let new_session = !self.connector_session_ready;
        let runtime_session_id = if new_session {
            new_id()
        } else {
            self.state.session_id.clone().ok_or("运行会话不存在")?
        };
        let context = ConnectorContext {
            workspace_id: self.state.workspace_id.clone(),
            root: self.state.root_path.clone(),
            generation: self.state.workspace_generation.clone(),
            permission_epoch: self.permission_epoch,
            runtime_session_id: runtime_session_id.clone(),
            run_id: new_id(),
            module_id: module_id.clone(),
        };
        if new_session {
            self.begin_module_tool_session(&context);
        }
        self.begin_module_tool_run(&context)?;
        let setup = (|| {
            if new_session {
                self.connector.initialize(&context)?;
                self.connector.new_session(&context)?;
            }
            self.connector.prompt(ConnectorPrompt {
                context: context.clone(),
                text: self.module_tool_prompt_text(&text, &context),
                modules,
            })
        })();
        if let Err(error) = setup {
            let stopped = self.stop_connector("任务启动失败");
            if stopped.is_ok() {
                self.state.connection.status = "error".into();
                self.state.connection.message = error.clone();
            }
            self.connector_session_ready = false;
            return Err(error);
        }
        self.lifecycle_context = Some(context.clone());
        self.connector_session_ready = true;
        self.state.session_id = Some(runtime_session_id);
        if new_session {
            self.state.provider_session_id = None;
        }
        self.push_message("user", &text);
        self.audit(
            "session/prompt",
            &format!(
                "接收任务；上下文范围：{}；已保存模块通过 Host 工具按需读取；不包含未保存草稿",
                module_id.as_deref().unwrap_or("工作区模块摘要")
            ),
            "user",
        )?;
        let message_id = self.push_message("assistant", "");
        self.state.run_status = RunStatus::Running;
        self.set_target_status(module_id.as_deref(), ModuleStatus::Running);
        self.active = Some(ActiveRun {
            pending_output: String::new(),
            message_id,
            module_id,
            context,
            last_seq: 0,
            connect_only: false,
        });
        Ok(())
    }

    fn stage_write(
        &mut self,
        module_id: &str,
        before: String,
        after: String,
        title: &str,
        actor: &str,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Err("Agent 正在运行，请先完成或取消当前任务".into());
        }
        self.stage_write_proposal(module_id, before, after, title, actor, ActionOrigin::User)
    }

    fn stage_write_proposal(
        &mut self,
        module_id: &str,
        before: String,
        after: String,
        title: &str,
        actor: &str,
        origin: ActionOrigin,
    ) -> Result<(), String> {
        validate_text(&after, MAX_TEXT)?;
        if self
            .state
            .approvals
            .iter()
            .any(|a| a.module_id.as_deref() == Some(module_id))
        {
            return Err("此文档已有待处理修改，请先批准或拒绝".into());
        }
        let module = self.module(module_id)?;
        let approval = Approval {
            id: new_id(),
            kind: "write_file".into(),
            title: title.into(),
            description: format!(
                "写入 {}；已绑定原文 SHA-256，外部修改会阻止应用。",
                module.file_path.as_deref().unwrap_or("文档")
            ),
            module_id: Some(module_id.into()),
            module_type: Some(ModuleType::Document),
            file_path: module.file_path.clone(),
            origin: origin.as_str().into(),
            epoch: self.permission_epoch,
            options: Vec::new(),
            scope: None,
            changes: None,
            proposal_id: None,
            context: self.active.as_ref().map(|run| run.context.clone()),
            revision: Some(content_revision(&before)),
            before: Some(before),
            after: Some(after),
        };
        self.state.approvals.push(approval);
        self.module_mut(module_id)?.status = ModuleStatus::WaitingApproval;
        self.state.run_status = RunStatus::WaitingApproval;
        self.audit(
            "permission/request",
            &format!("已生成文档 diff：{module_id}"),
            actor,
        )?;
        Ok(())
    }

    fn decide_approval(&mut self, id: &str, allow: bool) -> Result<(), String> {
        let index = self
            .state
            .approvals
            .iter()
            .position(|a| a.id == id)
            .ok_or("审批不存在或已经失效")?;
        let approval = self.state.approvals[index].clone();
        if approval.kind == "module_changes" {
            return self.decide_module_proposal(id, allow);
        }
        if approval.kind == "agent_permission" {
            return Err("Agent权限请求必须选择原始选项".into());
        }
        if allow {
            if approval.origin == "agent" {
                self.require_agent_write(&approval)?;
            } else if self.state.policy.is_none() {
                self.require_writes()?;
            }
            match approval.kind.as_str() {
                "create_module" => {
                    let kind = approval.module_type.ok_or("审批缺少模块类型")?;
                    let module_id = self.insert_module_with_approval(
                        kind,
                        approval.title.clone(),
                        approval.after.clone(),
                        Some(id),
                    )?;
                    self.audit(
                        "tool/create_module",
                        &format!("审批 {id} 创建模块 {module_id}"),
                        "host",
                    )?;
                }
                "write_file" => {
                    let module_id = approval.module_id.as_deref().ok_or("审批缺少模块 ID")?;
                    let module = self.module(module_id)?;
                    if module.module_type != ModuleType::Document
                        || module.file_path != approval.file_path
                    {
                        return Err("审批目标已变化，请拒绝后重新生成修改".into());
                    }
                    let path = approval.file_path.as_deref().ok_or("审批缺少文件路径")?;
                    let after = approval.after.as_deref().ok_or("审批缺少文件内容")?;
                    validate_text(after, MAX_TEXT)?;
                    let before_revision = approval
                        .revision
                        .as_deref()
                        .ok_or("审批缺少原文 revision")?;
                    let current = self.read_document(path)?;
                    if content_revision(&current) != before_revision {
                        return Err(
                            "文件 revision 冲突：原文已被外部修改，请拒绝旧审批后重新生成".into(),
                        );
                    }
                    self.connection.execute(
                        "INSERT INTO write_operations(id,module_id,path,before_revision,after_revision,status)
                         VALUES(?1,?2,?3,?4,?5,'pending')
                         ON CONFLICT(id) DO UPDATE SET status='pending'",
                        params![id,module_id,path,before_revision,content_revision(after)]
                    ).map_err(db_error)?;
                    self.storage_checkpoint("before_file_write")?;
                    let revision =
                        match atomic_write(&self.root, path, after, Some(before_revision)) {
                            Ok(revision) => revision,
                            Err(error) => {
                                self.storage_fault = Some(error.clone());
                                return Err(error);
                            }
                        };
                    self.storage_checkpoint("after_file_write")?;
                    let module = self.module_mut(module_id)?;
                    module.content = after.into();
                    module.revision = Some(revision.clone());
                    module.status = ModuleStatus::Attention;
                    self.audit(
                        "tool/write_file",
                        &format!("审批 {id} 写入 {path}；revision={revision}"),
                        "host",
                    )?;
                }
                _ => return Err("不支持的审批类型".into()),
            }
        } else if let Some(module_id) = &approval.module_id {
            if let Some(module) = self.state.modules.iter_mut().find(|m| &m.id == module_id) {
                module.status = ModuleStatus::Idle;
            }
        }
        self.state.approvals.remove(index);
        self.audit(
            if allow {
                "permission/approved"
            } else {
                "permission/rejected"
            },
            &format!(
                "用户{}审批 {id}：{}",
                if allow { "批准" } else { "拒绝" },
                approval.title
            ),
            "user",
        )?;
        if self.state.approvals.is_empty() && self.active.is_none() {
            self.state.run_status = RunStatus::Completed;
            for module in &mut self.state.modules {
                if module.status == ModuleStatus::WaitingApproval {
                    module.status = ModuleStatus::Idle;
                }
            }
            self.audit(
                "completed",
                if allow {
                    "已应用获批操作"
                } else {
                    "操作被拒绝，原文件保持不变"
                },
                "host",
            )?;
        }
        Ok(())
    }

    fn require_writes(&self) -> Result<(), String> {
        if self.state.permission_mode == PermissionMode::Restricted {
            Err("受限模式仅允许生成 diff，Host 已阻止写入；切换到逐次审批模式后才能批准".into())
        } else {
            Ok(())
        }
    }

    fn require_no_run(&self) -> Result<(), String> {
        if self.active.is_some()
            || self.state.run_status == RunStatus::Running
            || !self.state.approvals.is_empty()
        {
            Err("当前任务尚未结束，请先完成审批或取消，再开始新任务".into())
        } else {
            Ok(())
        }
    }

    fn insert_module(
        &mut self,
        kind: ModuleType,
        title: String,
        content: Option<String>,
    ) -> Result<String, String> {
        self.insert_module_with_approval(kind, title, content, None)
    }

    fn insert_module_with_approval(
        &mut self,
        kind: ModuleType,
        title: String,
        content: Option<String>,
        approval_id: Option<&str>,
    ) -> Result<String, String> {
        if self.state.modules.len() >= MAX_MODULES {
            return Err("模块数量已达上限".into());
        }
        let title = clean_title(&title)?;
        let id = new_id();
        let content = content.unwrap_or_else(|| {
            if kind == ModuleType::Document {
                format!("# {title}\n\n在这里开始写作。\n")
            } else {
                String::new()
            }
        });
        validate_text(&content, MAX_TEXT)?;
        let (file_path, revision) = if kind == ModuleType::Document {
            let path = format!("notes/{id}.md");
            let revision = content_revision(&content);
            (Some(path), Some(revision))
        } else {
            (None, None)
        };
        let layout = if self.state.modules.is_empty() {
            Layout {
                x: 0,
                y: 0,
                w: 12,
                h: 47,
            }
        } else if self.state.modules.len() == 1 {
            Layout {
                x: 12,
                y: 0,
                w: 12,
                h: 47,
            }
        } else {
            Layout {
                x: 0,
                y: self
                    .state
                    .modules
                    .iter()
                    .map(|m| m.layout.y + m.layout.h)
                    .max()
                    .unwrap_or(0),
                w: if kind == ModuleType::Dashboard {
                    24
                } else {
                    12
                },
                h: if kind == ModuleType::Dashboard {
                    34
                } else {
                    43
                },
            }
        };
        let module = WorkspaceModule {
            id: id.clone(),
            module_type: kind,
            title,
            status: ModuleStatus::Idle,
            layout,
            tasks: if kind == ModuleType::Planner {
                fixture_tasks()
            } else {
                Vec::new()
            },
            content,
            file_path,
            revision,
            module_revision: None,
            dashboard_config: if kind == ModuleType::Dashboard {
                Some(DashboardConfig::default())
            } else {
                None
            },
        };
        self.connection.execute("INSERT INTO create_operations(id,module_json,approval_id,status) VALUES(?1,?2,?3,'pending')",
            params![id,serde_json::to_string(&module).map_err(json_error)?,approval_id]).map_err(db_error)?;
        self.storage_checkpoint("before_create_file")?;
        if let Some(path) = module.file_path.as_deref() {
            if let Err(error) = atomic_write(&self.root, path, &module.content, None) {
                self.storage_fault = Some(error.clone());
                return Err(error);
            }
        }
        self.storage_checkpoint("after_create_file")?;
        self.state.modules.push(module);
        Ok(id)
    }

    fn module(&self, id: &str) -> Result<&WorkspaceModule, String> {
        self.state
            .modules
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| "模块不存在".into())
    }
    fn module_mut(&mut self, id: &str) -> Result<&mut WorkspaceModule, String> {
        self.state
            .modules
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or_else(|| "模块不存在".into())
    }
    fn set_target_status(&mut self, id: Option<&str>, status: ModuleStatus) {
        for module in &mut self.state.modules {
            if Some(module.id.as_str()) == id || module.module_type == ModuleType::Conversation {
                module.status = status;
            }
        }
    }
    fn read_document(&self, path: &str) -> Result<String, String> {
        let path = contained_path(&self.root, path)?;
        let size = fs::metadata(&path).map_err(io_error)?.len();
        if size > MAX_TEXT as u64 {
            return Err("文档超过 P0 的 1 MB 上限".into());
        }
        fs::read_to_string(path).map_err(io_error)
    }
    fn refresh_document(&self, module: &mut WorkspaceModule) -> Result<(), String> {
        if module.module_type == ModuleType::Document {
            let relative = module.file_path.as_deref().ok_or("文档缺少文件路径")?;
            match self.read_document(relative) {
                Ok(content) => {
                    module.revision = Some(content_revision(&content));
                    module.content = content;
                }
                Err(error) => {
                    // Preserve visible context when an external editor moves/removes a file.
                    module.status = ModuleStatus::Error;
                    module.revision = None;
                    if module.content.is_empty() {
                        module.content = format!("无法读取本地文档：{error}");
                    }
                }
            }
        }
        Ok(())
    }
    fn push_message(&mut self, role: &str, text: &str) -> String {
        let id = new_id();
        self.state.messages.push(Message {
            id: id.clone(),
            role: role.into(),
            text: text.into(),
            timestamp: now(),
        });
        id
    }
    fn audit(&mut self, kind: &str, message: &str, actor: &str) -> Result<(), String> {
        self.pending_audit.push(PendingAudit {
            kind: kind.into(),
            message: self.redact_tool_credentials(message),
            actor: actor.into(),
            session_id: self.state.session_id.clone(),
            timestamp: now(),
            context: self.active.as_ref().map(|run| run.context.clone()),
            connector_seq: None,
        });
        Ok(())
    }

    fn persist(&mut self) -> Result<(), String> {
        let result = self.persist_inner();
        if let Err(error) = &result {
            self.storage_fault = Some(error.clone());
            self.active = None;
            if let Err(stop) = self.connector.shutdown() {
                self.state.connection.status = "stopping".into();
                self.state.connection.message = format!("持久化失败且进程停止未确认：{stop}");
            } else {
                self.state.connection.status = "error".into();
                self.state.connection.message =
                    "工作区保存失败；Agent 已回收，需重新打开恢复".into();
            }
            self.connector_session_ready = false;
            self.state.run_status = RunStatus::Failed;
        }
        result
    }

    fn persist_inner(&mut self) -> Result<(), String> {
        self.workspace_lock.ensure_valid()?;
        reconcile_module_activity(&mut self.state, self.active.as_ref());
        for module in &mut self.state.modules {
            module.module_revision = Some(module_tools::module_revision(module));
        }
        self.state.updated_at = now();
        let pending = PendingCommit {
            state: self.state.clone(),
            events: self.pending_audit.clone(),
            recovered_operation_states: self.recovered_operation_states.clone(),
        };
        self.connection
            .execute(
                "INSERT INTO metadata(key,value) VALUES('pending_commit',?1)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [serde_json::to_string(&pending).map_err(json_error)?],
            )
            .map_err(db_error)?;
        self.storage_checkpoint("after_pending_commit")?;
        for module in &self.state.modules {
            let json = serde_json::to_string_pretty(module).map_err(json_error)?;
            write_metadata(
                &self.root,
                &format!(".workspace/modules/{}.json", module.id),
                &json,
            )?;
        }
        let manifest = WorkspaceManifest {
            schema_version: 1,
            name: self.state.name.clone(),
            module_ids: self.state.modules.iter().map(|m| m.id.clone()).collect(),
            permission_mode: self.state.permission_mode,
            allow_overlap: self.state.allow_overlap,
            agent: self.state.agent.clone(),
            workspace_id: self.state.workspace_id.clone(),
        };
        write_metadata(
            &self.root,
            ".workspace/workspace.json",
            &serde_json::to_string_pretty(&manifest).map_err(json_error)?,
        )?;
        self.storage_checkpoint("after_manifests")?;
        let mut saved = self.state.clone();
        saved.events.clear(); // All audit history lives in the append-only events table.
        let snapshot = serde_json::to_string(&saved).map_err(json_error)?;
        let transaction = self.connection.transaction().map_err(db_error)?;
        transaction.execute("INSERT INTO metadata(key,value) VALUES('snapshot',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [snapshot]).map_err(db_error)?;
        transaction
            .execute("DELETE FROM modules", [])
            .map_err(db_error)?;
        for module in &self.state.modules {
            transaction
                .execute(
                    "INSERT INTO modules(id,data) VALUES(?1,?2)",
                    params![
                        module.id,
                        serde_json::to_string(module).map_err(json_error)?
                    ],
                )
                .map_err(db_error)?;
        }
        transaction
            .execute("DELETE FROM messages", [])
            .map_err(db_error)?;
        for (position, message) in self.state.messages.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO messages(id,position,data) VALUES(?1,?2,?3)",
                    params![
                        message.id,
                        position as i64,
                        serde_json::to_string(message).map_err(json_error)?
                    ],
                )
                .map_err(db_error)?;
        }
        transaction
            .execute("DELETE FROM approvals", [])
            .map_err(db_error)?;
        for approval in &self.state.approvals {
            transaction
                .execute(
                    "INSERT INTO approvals(id,data) VALUES(?1,?2)",
                    params![
                        approval.id,
                        serde_json::to_string(approval).map_err(json_error)?
                    ],
                )
                .map_err(db_error)?;
        }
        if let Some(session) = &self.state.session_id {
            transaction.execute("INSERT INTO sessions(id,status,updated_at) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET status=excluded.status,updated_at=excluded.updated_at",
                params![session, serde_json::to_string(&self.state.run_status).map_err(json_error)?, self.state.updated_at]).map_err(db_error)?;
            if let Some(provider) = &self.state.provider_session_id {
                // Recovery refreshes the live generation before committing. A
                // pending session/new still belongs to its original envelope.
                let context = self
                    .pending_audit
                    .iter()
                    .find(|event| {
                        event.kind == "session/new" && event.session_id.as_ref() == Some(session)
                    })
                    .and_then(|event| event.context.as_ref());
                let workspace_id =
                    context.map_or(&self.state.workspace_id, |context| &context.workspace_id);
                let root = context.map_or(&self.state.root_path, |context| &context.root);
                let generation = context.map_or(&self.state.workspace_generation, |context| {
                    &context.generation
                });
                let epoch =
                    context.map_or(self.permission_epoch, |context| context.permission_epoch);
                transaction.execute("INSERT INTO provider_sessions(runtime_session_id,provider_session_id,workspace_id,root,generation,permission_epoch,agent_id)
                    VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(runtime_session_id) DO NOTHING",
                    params![session,provider,workspace_id,root,generation,i64::try_from(epoch).map_err(|_| "权限代次超出存储范围")?,self.state.agent.id]).map_err(db_error)?;
            }
        }
        for event in &self.pending_audit {
            let context_json = event
                .context
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(json_error)?;
            let connector_seq = event
                .connector_seq
                .map(i64::try_from)
                .transpose()
                .map_err(|_| "Agent 事件序号超出存储范围")?;
            transaction.execute("INSERT INTO events(kind,message,timestamp,session_id,actor,context_json,connector_seq) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![event.kind,event.message,event.timestamp,event.session_id,event.actor,context_json,connector_seq]).map_err(db_error)?;
        }
        for (id, status) in &self.recovered_operation_states {
            transaction
                .execute(
                    "UPDATE write_operations SET status=?1 WHERE id=?2",
                    params![status, id],
                )
                .map_err(db_error)?;
        }
        for module in &self.state.modules {
            transaction.execute("UPDATE create_operations SET status='applied' WHERE id=?1 AND status='pending'",[&module.id]).map_err(db_error)?;
        }
        transaction
            .execute(
                "UPDATE create_operations SET status='not_applied' WHERE status='pending'",
                [],
            )
            .map_err(db_error)?;
        // A completed/rejected approval can no longer be replayed after recovery.
        for approval in &self.state.approvals {
            transaction.execute("UPDATE write_operations SET status='not_applied' WHERE id=?1 AND status='pending'",[&approval.id]).map_err(db_error)?;
        }
        transaction
            .execute(
                "UPDATE write_operations SET status='applied' WHERE status='pending'",
                [],
            )
            .map_err(db_error)?;
        transaction
            .execute("DELETE FROM metadata WHERE key='pending_commit'", [])
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        self.pending_audit.clear();
        self.recovered_operation_states.clear();
        self.state.events = load_events(&self.connection)?;
        Ok(())
    }

    fn require_healthy_storage(&self) -> Result<(), String> {
        match &self.storage_fault {
            Some(error) => Err(format!(
                "工作区保存未完成，请关闭并重新打开以恢复；未报告成功：{error}"
            )),
            None => Ok(()),
        }
    }

    fn storage_checkpoint(&mut self, _point: &'static str) -> Result<(), String> {
        #[cfg(test)]
        if self.failpoint == Some(_point) {
            self.failpoint = None;
            let error = format!("模拟持久化中断：{_point}");
            self.storage_fault = Some(error.clone());
            self.active = None;
            self.state.run_status = RunStatus::Failed;
            return Err(error);
        }
        Ok(())
    }

    fn recover_write_operations(&mut self) -> Result<(), String> {
        let operations = {
            let mut stmt = self.connection.prepare("SELECT id,module_id,path,before_revision,after_revision FROM write_operations WHERE status='pending'").map_err(db_error)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })
                .map_err(db_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
        };
        for (id, module_id, path, before, after) in operations {
            if self
                .recovered_operation_states
                .iter()
                .any(|(known, _)| known == &id)
            {
                continue;
            }
            if !self.state.approvals.iter().any(|a| a.id == id) {
                continue;
            }
            let content = self.read_document(&path).ok();
            let revision = content.as_deref().map(content_revision);
            if revision.as_deref() == Some(&after) {
                let module = self.module_mut(&module_id)?;
                if module.file_path.as_deref() != Some(&path) {
                    return Err("恢复记录的文档目标已变化".into());
                }
                module.content = content.unwrap();
                module.revision = Some(after);
                module.status = ModuleStatus::Attention;
                self.state.approvals.retain(|a| a.id != id);
                self.audit(
                    "storage/recovered",
                    &format!("审批 {id} 的文件已写入；完成状态恢复，不重复写入"),
                    "host",
                )?;
                if self.state.approvals.is_empty() {
                    self.state.run_status = RunStatus::Completed;
                }
            } else {
                let status = if revision.as_deref() == Some(&before) {
                    "not_applied"
                } else {
                    "conflict"
                };
                self.recovered_operation_states
                    .push((id.clone(), status.into()));
                self.audit(
                    "storage/recovery_required",
                    &format!("审批 {id} 未完成（{status}），保留文件与提案，需用户明确重试或拒绝"),
                    "host",
                )?;
            }
        }
        Ok(())
    }

    fn recover_create_operations(&mut self) -> Result<(), String> {
        let records = {
            let mut stmt = self
                .connection
                .prepare(
                    "SELECT module_json,approval_id FROM create_operations WHERE status='pending'",
                )
                .map_err(db_error)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                })
                .map_err(db_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
        };
        for (json, approval_id) in records {
            let module: WorkspaceModule = serde_json::from_str(&json).map_err(json_error)?;
            validate_module(&module)?;
            if self.state.modules.iter().any(|m| m.id == module.id) {
                continue;
            }
            let applied = match &module.file_path {
                Some(path) => self
                    .read_document(path)
                    .map(|text| content_revision(&text) == content_revision(&module.content))
                    .unwrap_or(false),
                None => true,
            };
            if applied {
                if let Some(id) = approval_id {
                    self.state.approvals.retain(|a| a.id != id);
                }
                self.audit(
                    "storage/recovered",
                    &format!(
                        "模块 {} 创建已生效，恢复同一模块与文件，不重复创建",
                        module.id
                    ),
                    "host",
                )?;
                self.state.modules.push(module);
                if self.state.approvals.is_empty() {
                    self.state.run_status = RunStatus::Completed;
                }
            } else {
                self.audit(
                    "storage/recovery_required",
                    &format!("模块 {} 创建未完成或文件已变化，保留现状与审批", module.id),
                    "host",
                )?;
            }
        }
        Ok(())
    }
}

impl HostWorkspaceTools for Kernel {
    fn module_context(&self) -> Result<Vec<ModuleContext>, String> {
        Ok(self
            .snapshot()?
            .modules
            .into_iter()
            .map(|module| ModuleContext {
                id: module.id,
                module_type: module.module_type,
                title: module.title,
                content: module.content,
                revision: module.revision,
            })
            .collect())
    }

    fn propose_tool(
        &mut self,
        context: &ConnectorContext,
        proposal: WorkspaceToolIntent,
    ) -> Result<(), String> {
        self.workspace_lock.ensure_valid()?;
        self.validate_connector_context(context)?;
        self.require_healthy_storage()?;
        if self
            .active
            .as_ref()
            .is_none_or(|run| run.context != *context)
        {
            return Err("Agent 运行已结束，此工具提案已失效".into());
        }
        let actor = self.state.agent.id.clone();
        match proposal {
            WorkspaceToolIntent::ModuleProposal {
                module_type,
                title,
                content,
            } => {
                if self.state.modules.len() >= MAX_MODULES {
                    return Err("模块数量已达上限".into());
                }
                let title = clean_title(&title)?;
                if let Some(content) = &content {
                    validate_text(content, MAX_TEXT)?;
                }
                self.state.approvals.push(Approval {
                    id: new_id(),
                    kind: "create_module".into(),
                    title: title.clone(),
                    description: format!(
                        "创建内置{}模块。{}",
                        module_name(module_type),
                        if module_type == ModuleType::Document {
                            "批准后会在 notes/ 下新增 Markdown 文件。"
                        } else {
                            "使用本地 fixture 数据，无终端或网络权限。"
                        }
                    ),
                    module_id: None,
                    module_type: Some(module_type),
                    file_path: None,
                    before: None,
                    after: content,
                    revision: None,
                    origin: "agent".into(),
                    epoch: self.permission_epoch,
                    options: Vec::new(),
                    scope: None,
                    changes: None,
                    proposal_id: None,
                    context: Some(context.clone()),
                });
                self.state.run_status = RunStatus::WaitingApproval;
                self.set_target_status(context.module_id.as_deref(), ModuleStatus::WaitingApproval);
                self.audit(
                    "workspace.module.propose",
                    &format!("提议创建：{title}"),
                    &actor,
                )?;
                self.audit("permission/request", "模块提议等待用户决定", "host")?;
            }
            WorkspaceToolIntent::WriteProposal {
                module_id,
                title,
                before_revision,
                content,
            } => {
                let module = self.module(&module_id)?;
                if module.module_type != ModuleType::Document {
                    return Err("工具提案目标不是文档模块".into());
                }
                let before =
                    self.read_document(module.file_path.as_deref().ok_or("文档没有文件路径")?)?;
                if content_revision(&before) != before_revision {
                    return Err(
                        "文件 revision 冲突：Agent 运行期间文档已修改，请重新生成提案".into(),
                    );
                }
                self.stage_write_proposal(
                    &module_id,
                    before,
                    content,
                    &clean_title(&title)?,
                    &actor,
                    ActionOrigin::Agent,
                )?;
                self.set_target_status(context.module_id.as_deref(), ModuleStatus::WaitingApproval);
            }
        }
        Ok(())
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        let _ = self.connector.shutdown();
    }
}

/// Running and waiting are live activity, not durable module facts. Reconcile
/// both snapshots and commits so old manifests cannot resurrect a busy badge.
/// Terminal content/error/offline statuses are preserved when there is no work.
fn reconcile_module_activity(state: &mut WorkspaceSnapshot, active: Option<&ActiveRun>) {
    let run = active.filter(|run| !run.connect_only);
    let mut waiting = HashSet::new();
    for approval in &state.approvals {
        if let Some(id) = &approval.module_id {
            waiting.insert(id.as_str());
        }
        if let Some(id) = approval
            .context
            .as_ref()
            .and_then(|context| context.module_id.as_deref())
        {
            waiting.insert(id);
        }
    }
    for module in &mut state.modules {
        let conversation = module.module_type == ModuleType::Conversation;
        let has_approval =
            waiting.contains(module.id.as_str()) || conversation && !state.approvals.is_empty();
        let is_running = run.is_some_and(|run| {
            conversation || run.module_id.as_deref() == Some(module.id.as_str())
        });
        if has_approval {
            module.status = ModuleStatus::WaitingApproval;
        } else if is_running {
            module.status = ModuleStatus::Running;
        } else if matches!(
            module.status,
            ModuleStatus::Running | ModuleStatus::WaitingApproval
        ) {
            module.status = ModuleStatus::Idle;
        }
    }
}

fn default_state(root: &Path) -> WorkspaceSnapshot {
    WorkspaceSnapshot { schema_version: 1, name: "个人工作区".into(), root_path: root.to_string_lossy().into_owned(),
        modules: Vec::new(), messages: vec![Message { id: new_id(), role: "assistant".into(),
            text: "你好，这里是你的本地 Agent 工作区。试着说「帮我做本周计划」，我会生成一个待审批的规划模块。也可以选择文档，让我先生成修改预览。当前由 Mock Agent 演示全部流程。".into(), timestamp: now() }],
        events: Vec::new(), approvals: Vec::new(), run_status: RunStatus::Idle, session_id: None,
        workspace_id: new_id(), provider_session_id: None, policy:None,connection:ConnectionState::default(), module_proposals: Vec::new(),
        permission_mode: PermissionMode::Ask, allow_overlap: false, updated_at: now(), workspace_generation: new_id(),
        agent: AgentDescriptor { provider: Some("mock".into()), id: "mock-agent".into(), name: "Mock Agent".into(), transport: "mock".into(),
            command: String::new(), args: Vec::new(), env: BTreeMap::new(), cwd: String::new(), probe_status: "ready".into() } }
}
fn initial_document() -> &'static str {
    "# 让想法有自己的位置\n\n这是一个本地优先的个人 Agent 工作区原型。文档存放在工作区 notes/ 文件夹，模块与布局会自动保存。\n\n## 试试这些操作\n\n1. 输入「帮我做本周计划」，批准后创建规划模块。\n2. 拖动标题栏调整位置，拖动右下角调整大小。\n3. 双击标题进入聚焦，按 Esc 返回画布。\n4. 输入「修改文档」，审阅差异并批准或拒绝。\n5. 输入「模拟失败」或「执行一个长任务」测试恢复和取消。\n\n## 权限与数据\n\n- Ask：每次应用 Agent 提议都需要审批。\n- Restricted：只能生成差异，不能应用文件写入。\n- 当前使用本地 Mock，不访问网络，不执行终端。\n- 退出后重新打开，模块、布局和事件会恢复。\n"
}
fn fixture_tasks() -> Vec<Task> {
    [
        ("明确本周目标与交付物", "周一 09:00", "规划", true),
        ("整理资料，建立初稿结构", "周二 10:00", "专注", false),
        ("完成核心工作并检查进度", "周三 14:00", "执行", false),
        ("审阅、交付与周复盘", "周五 16:00", "复盘", false),
    ]
    .into_iter()
    .map(|(title, time, tag, done)| Task {
        id: new_id(),
        title: title.into(),
        done,
        time: time.into(),
        tag: tag.into(),
    })
    .collect()
}
fn module_name(kind: ModuleType) -> &'static str {
    match kind {
        ModuleType::Conversation => "对话",
        ModuleType::Planner => "时间规划",
        ModuleType::Document => "Markdown 文档",
        ModuleType::Dashboard => "数据看板",
    }
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn new_id() -> String {
    Uuid::new_v4().to_string()
}
fn host_actor() -> String {
    "host".into()
}
fn io_error(error: std::io::Error) -> String {
    format!("本地文件操作失败：{error}")
}
fn db_error(error: rusqlite::Error) -> String {
    format!("工作区数据库错误：{error}")
}
fn json_error(error: serde_json::Error) -> String {
    format!("工作区数据格式错误：{error}")
}
fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, String> {
    if fs::metadata(path).map_err(io_error)?.len() > 5_000_000 {
        return Err("工作区数据文件过大".into());
    }
    serde_json::from_str(&fs::read_to_string(path).map_err(io_error)?).map_err(json_error)
}
fn clean_title(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 120 || title.chars().any(char::is_control) {
        return Err("标题需为 1–120 个字符，不能包含控制字符".into());
    }
    Ok(title.into())
}
fn validate_text(text: &str, limit: usize) -> Result<(), String> {
    if text.len() > limit || text.contains('\0') {
        return Err(format!("内容无效或超过 {} 字节上限", limit));
    }
    Ok(())
}
fn validate_id(id: &str) -> Result<(), String> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| "模块 ID 必须是 UUID".into())
}
fn validate_layout(layout: &Layout) -> Result<(), String> {
    if layout.w < 6
        || layout.w > 24
        || layout.x > 24 - layout.w
        || layout.h < 24
        || layout.h > 120
        || layout.y > 50_000
    {
        Err("布局超出 24 列画布范围；最小尺寸为 6 × 24，最大高度为 120".into())
    } else {
        Ok(())
    }
}
fn validate_module(module: &WorkspaceModule) -> Result<(), String> {
    validate_id(&module.id)?;
    clean_title(&module.title)?;
    validate_layout(&module.layout)?;
    validate_text(&module.content, MAX_TEXT)?;
    if module.tasks.len() > 500 {
        return Err("任务数量超限".into());
    }
    if let Some(path) = &module.file_path {
        if !path.starts_with("notes/") || !path.ends_with(".md") {
            return Err("P0 文档只允许 notes/ 内的 Markdown 文件".into());
        }
    }
    Ok(())
}
fn normalize_agent(
    root: &Path,
    previous_root: Option<&Path>,
    mut agent: AgentDescriptor,
) -> Result<AgentDescriptor, String> {
    if agent.provider.is_none() {
        agent.provider = Some(
            if agent.transport == "mock" {
                "mock"
            } else {
                "hermes"
            }
            .into(),
        );
    }
    let requested = Path::new(&agent.cwd);
    if requested.is_absolute() {
        // Only startup may rebase a path under the root recorded in this
        // workspace's snapshot. Never infer the old root from an arbitrary cwd.
        if let Some(previous) = previous_root.filter(|path| path.is_absolute() && *path != root) {
            if let Ok(relative) = requested.strip_prefix(previous) {
                agent.cwd = if relative.as_os_str().is_empty() {
                    String::new()
                } else {
                    portable_relative_path(relative)
                };
            }
        }
    }
    let resolved = resolve_agent_cwd(root, &agent.cwd)?;
    let relative = resolved
        .strip_prefix(root)
        .map_err(|_| "Agent 工作目录必须位于当前工作区内".to_string())?;
    agent.cwd = if relative.as_os_str().is_empty() {
        // Empty meant workspace root in 0.1.0 as well; keep manifests readable
        // if the user rolls back to the preserved previous application.
        String::new()
    } else {
        portable_relative_path(relative)
    };
    validate_agent(root, &agent)?;
    Ok(agent)
}

fn portable_relative_path(relative: &Path) -> String {
    #[cfg(windows)]
    {
        relative.to_string_lossy().replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        relative.to_string_lossy().into_owned()
    }
}

fn resolve_agent_cwd(root: &Path, cwd: &str) -> Result<PathBuf, String> {
    let path = if cwd.is_empty() || cwd == "." {
        root.to_path_buf()
    } else if Path::new(cwd).is_absolute() {
        fs::canonicalize(cwd).map_err(io_error)?
    } else {
        contained_path(root, cwd)?
    };
    if !path.starts_with(root) || !path.is_dir() {
        return Err("Agent 工作目录必须位于当前工作区内".into());
    }
    Ok(path)
}

fn validate_agent(root: &Path, agent: &AgentDescriptor) -> Result<(), String> {
    clean_title(&agent.name)?;
    if !matches!(
        agent.provider.as_deref(),
        None | Some("mock" | "hermes" | "claude_code" | "codex")
    ) {
        return Err("不支持的 Agent provider".into());
    }
    if agent.transport != "mock" && agent.transport != "stdio" {
        return Err("Agent transport 只支持 mock / stdio 描述".into());
    }
    if agent.id.is_empty()
        || agent.id.len() > 120
        || agent.command.len() > 4096
        || agent.args.len() > 64
    {
        return Err("Agent descriptor 超出长度限制".into());
    }
    reject_secret(&agent.command)?;
    for arg in &agent.args {
        validate_text(arg, 4096)?;
        reject_secret(arg)?;
    }
    resolve_agent_cwd(root, &agent.cwd)?;
    let allowed = [
        "PATH", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "TERM", "NO_COLOR",
    ];
    for (key, value) in &agent.env {
        if !allowed.contains(&key.as_str()) {
            return Err(format!(
                "环境变量 {key} 不在允许列表；P0 不保存 API key、token 或其他明文密钥"
            ));
        }
        validate_text(value, 8192)?;
        reject_secret(value)?;
        if value.contains('\n') || value.contains('\r') {
            return Err("环境变量不能包含换行".into());
        }
    }
    Ok(())
}
fn reject_secret(value: &str) -> Result<(), String> {
    let lower = value.to_ascii_lowercase();
    if [
        "api_key",
        "apikey",
        "api-key",
        "--token",
        "--secret",
        "token=",
        "secret=",
        "bearer ",
        "sk-",
        "ghp_",
        "github_pat_",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        Err("P0 descriptor 不能包含明文密钥或凭据参数；请删除密钥配置".into())
    } else {
        Ok(())
    }
}
fn load_events(connection: &Connection) -> Result<Vec<AgentEvent>, String> {
    let mut statement = connection.prepare(&format!("SELECT seq,kind,message,timestamp,session_id,actor,context_json,connector_seq FROM events ORDER BY seq DESC LIMIT {RECENT_EVENTS}")).map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            let context_json: Option<String> = row.get(6)?;
            let context = context_json
                .map(|json| serde_json::from_str::<ConnectorContext>(&json))
                .transpose()
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
            Ok(AgentEvent {
                seq: row.get(0)?,
                kind: row.get(1)?,
                message: row.get(2)?,
                timestamp: row.get(3)?,
                session_id: row.get(4)?,
                actor: row.get(5)?,
                context,
                connector_seq: row
                    .get::<_, Option<i64>>(7)?
                    .map(|value| {
                        u64::try_from(value)
                            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(7, value))
                    })
                    .transpose()?,
            })
        })
        .map_err(db_error)?;
    let mut events: Vec<AgentEvent> = rows.collect::<Result<_, _>>().map_err(db_error)?;
    events.reverse();
    Ok(events)
}

/// Resolve each existing component, checking symlinks before creating or writing.
/// Absolute paths and lexical traversal are never accepted by the file gateway.
fn contained_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    #[cfg(windows)]
    crate::platform::windows::paths::validate_relative_path(relative)?;
    if relative.is_empty() || relative.contains('\0') || relative.contains('\\') {
        return Err("无效的工作区相对路径".into());
    }
    let mut target = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(name) = component else {
            return Err("文件路径必须位于工作区内，禁止绝对路径和 ..".into());
        };
        target.push(name);
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                #[cfg(windows)]
                crate::platform::windows::paths::reject_reparse(&target)?;
                target = fs::canonicalize(&target).map_err(io_error)?;
                #[cfg(windows)]
                let contained = crate::platform::windows::paths::is_within(root, &target);
                #[cfg(not(windows))]
                let contained = target.starts_with(root);
                if !contained {
                    return Err(format!("路径或软链接越过工作区边界：{relative}"));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(io_error(error)),
        }
    }
    #[cfg(windows)]
    let contained = crate::platform::windows::paths::is_within(root, &target);
    #[cfg(not(windows))]
    let contained = target.starts_with(root);
    if !contained {
        return Err(format!("路径越过工作区边界：{relative}"));
    }
    Ok(target)
}
fn content_revision(content: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(content.as_bytes()))
}

fn atomic_write(
    root: &Path,
    relative: &str,
    content: &str,
    expected: Option<&str>,
) -> Result<String, String> {
    // Manifests contain JSON-escaped Markdown; document entry points enforce 1 MB.
    validate_text(content, 5_000_000)?;
    let target = contained_path(root, relative)?;
    let parent = target.parent().ok_or("文件缺少父目录")?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let target = contained_path(root, relative)?;
    let current = match fs::read_to_string(&target) {
        Ok(text) => Some(content_revision(&text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(io_error(error)),
    };
    if current.as_deref() != expected {
        return Err(
            "文件 revision 冲突：原文已被外部修改或删除。请拒绝此审批，刷新后重新生成 diff".into(),
        );
    }
    let parent = target.parent().ok_or("文件缺少父目录")?;
    let temp = parent.join(format!(".pixel-{}.tmp", new_id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(io_error)?;
        file.write_all(content.as_bytes()).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        // Check again immediately before commit to catch external edits during preparation.
        let verified = contained_path(root, relative)?;
        if verified != target {
            return Err("写入期间文件路径发生变化".into());
        }
        let latest = match fs::read_to_string(&verified) {
            Ok(text) => Some(content_revision(&text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(io_error(error)),
        };
        if latest.as_deref() != expected {
            return Err("文件 revision 冲突：写入期间原文已变化".into());
        }
        crate::platform::commit_replace(&temp, &verified).map_err(io_error)?;
        Ok(content_revision(content))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_metadata(root: &Path, relative: &str, content: &str) -> Result<(), String> {
    let path = contained_path(root, relative)?;
    let existing = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(io_error(error)),
    };
    if existing.as_deref() == Some(content) {
        return Ok(());
    }
    atomic_write(
        root,
        relative,
        content,
        existing.as_deref().map(content_revision).as_deref(),
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        path: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                path: std::env::temp_dir().join(format!("pixel-workspace-test-{}", new_id())),
            }
        }
        fn open(&self) -> Kernel {
            Kernel::open(&self.path).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
    fn prompt(kernel: &mut Kernel, text: &str) {
        kernel
            .dispatch(WorkspaceAction::Prompt {
                text: text.into(),
                module_id: None,
            })
            .unwrap();
    }
    fn drain(kernel: &mut Kernel) {
        for _ in 0..1000 {
            if kernel.tick().unwrap().is_none() {
                return;
            }
        }
        panic!("Mock run did not finish");
    }
    fn document(kernel: &Kernel) -> WorkspaceModule {
        kernel
            .snapshot()
            .unwrap()
            .modules
            .into_iter()
            .find(|m| m.module_type == ModuleType::Document)
            .unwrap()
    }
    #[test]
    fn persistence_restores_layout_tasks_and_file_authority() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        kernel
            .dispatch(WorkspaceAction::CreateModule {
                module_type: ModuleType::Planner,
                title: "本周".into(),
            })
            .unwrap();
        let planner = kernel.state.modules.last().unwrap().clone();
        kernel
            .dispatch(WorkspaceAction::SetLayouts {
                layouts: vec![LayoutChange {
                    id: planner.id.clone(),
                    layout: Layout {
                        x: 4,
                        y: 90,
                        w: 14,
                        h: 30,
                    },
                }],
            })
            .unwrap();
        kernel
            .dispatch(WorkspaceAction::ToggleTask {
                module_id: planner.id.clone(),
                task_id: planner.tasks[1].id.clone(),
            })
            .unwrap();
        let doc = document(&kernel);
        let max_seq = kernel.state.events.last().unwrap().seq;
        drop(kernel);
        fs::write(
            fixture.path.join(doc.file_path.unwrap()),
            "# External editor\n",
        )
        .unwrap();
        let kernel = fixture.open();
        let restored = kernel.module(&planner.id).unwrap();
        assert_eq!(restored.layout.x, 4);
        assert!(restored.tasks[1].done);
        assert_eq!(document(&kernel).content, "# External editor\n");
        assert!(kernel.state.events.last().unwrap().seq > max_seq);
    }
    #[test]
    fn mock_stream_propose_accept_and_event_chain() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        prompt(&mut kernel, "帮我做本周计划");
        assert_eq!(kernel.state.run_status, RunStatus::Running);
        assert!(kernel
            .dispatch(WorkspaceAction::Prompt {
                text: "second".into(),
                module_id: None
            })
            .is_err());
        kernel.tick().unwrap();
        assert!(!kernel.state.messages.last().unwrap().text.is_empty());
        drain(&mut kernel);
        assert_eq!(kernel.state.run_status, RunStatus::WaitingApproval);
        assert_eq!(kernel.state.modules.len(), 3);
        let id = kernel.state.approvals[0].id.clone();
        kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: id.clone(),
                allow: true,
            })
            .unwrap();
        assert_eq!(kernel.state.modules.len(), 4);
        assert_eq!(kernel.state.run_status, RunStatus::Completed);
        assert!(kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: id,
                allow: true
            })
            .is_err());
        for kind in [
            "initialize",
            "session/new",
            "session/prompt",
            "session/update",
            "permission/request",
            "permission/approved",
            "completed",
        ] {
            assert!(
                kernel.state.events.iter().any(|e| e.kind == kind),
                "missing {kind}"
            );
        }
    }
    #[test]
    fn reject_write_leaves_original_and_pending_survives_restart() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let before = document(&kernel).content;
        prompt(&mut kernel, "修改文档");
        drain(&mut kernel);
        let id = kernel.state.approvals[0].id.clone();
        drop(kernel);
        let mut kernel = fixture.open();
        assert_eq!(kernel.state.run_status, RunStatus::WaitingApproval);
        kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: id,
                allow: false,
            })
            .unwrap();
        assert_eq!(document(&kernel).content, before);
        prompt(&mut kernel, "修改文档");
        drain(&mut kernel);
        let approval = kernel.state.approvals[0].clone();
        kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: approval.id,
                allow: true,
            })
            .unwrap();
        assert_eq!(document(&kernel).content, approval.after.unwrap());
    }
    #[test]
    fn stale_revision_and_restricted_mode_block_frontend_bypass() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        prompt(&mut kernel, "修改文档");
        drain(&mut kernel);
        let approval = kernel.state.approvals[0].clone();
        kernel
            .dispatch(WorkspaceAction::SetPermissionMode {
                mode: PermissionMode::Restricted,
            })
            .unwrap();
        assert!(kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: approval.id.clone(),
                allow: true
            })
            .is_err());
        assert!(kernel
            .dispatch(WorkspaceAction::CreateModule {
                module_type: ModuleType::Document,
                title: "blocked".into()
            })
            .is_err());
        kernel
            .dispatch(WorkspaceAction::SetPermissionMode {
                mode: PermissionMode::Ask,
            })
            .unwrap();
        let path = fixture.path.join(approval.file_path.unwrap());
        fs::write(&path, "external change").unwrap();
        assert!(kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: approval.id.clone(),
                allow: true
            })
            .unwrap_err()
            .contains("revision"));
        assert_eq!(fs::read_to_string(path).unwrap(), "external change");
        assert_eq!(kernel.state.approvals.len(), 1);
        kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: approval.id,
                allow: false,
            })
            .unwrap();
    }
    #[test]
    fn direct_editor_save_is_revision_bound_and_requires_approval() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let doc = document(&kernel);
        kernel
            .dispatch(WorkspaceAction::SetPermissionMode {
                mode: PermissionMode::Restricted,
            })
            .unwrap();
        kernel
            .dispatch(WorkspaceAction::EditDocument {
                module_id: doc.id.clone(),
                content: "# A manual draft\n".into(),
                revision: doc.revision.clone(),
            })
            .unwrap();
        assert_eq!(document(&kernel).content, doc.content);
        let approval = kernel.state.approvals[0].clone();
        assert!(kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: approval.id.clone(),
                allow: true
            })
            .is_err());
        kernel
            .dispatch(WorkspaceAction::SetPermissionMode {
                mode: PermissionMode::Ask,
            })
            .unwrap();
        kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: approval.id,
                allow: true,
            })
            .unwrap();
        assert_eq!(document(&kernel).content, "# A manual draft\n");
        assert!(kernel
            .dispatch(WorkspaceAction::EditDocument {
                module_id: doc.id,
                content: "stale edit".into(),
                revision: doc.revision
            })
            .is_err());
        assert_eq!(document(&kernel).content, "# A manual draft\n");
    }
    #[test]
    fn file_manifests_can_rebuild_module_index_after_database_loss() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        kernel
            .dispatch(WorkspaceAction::CreateModule {
                module_type: ModuleType::Planner,
                title: "保留的计划".into(),
            })
            .unwrap();
        let planner_id = kernel.state.modules.last().unwrap().id.clone();
        let doc = document(&kernel);
        drop(kernel);
        fs::remove_file(fixture.path.join(".workspace/workspace.sqlite3")).unwrap();
        let kernel = fixture.open();
        assert_eq!(kernel.module(&planner_id).unwrap().title, "保留的计划");
        assert_eq!(document(&kernel).content, doc.content);
        assert_eq!(kernel.state.modules.len(), 4);
    }
    #[test]
    fn cancel_failure_and_interrupted_restart_allow_future_work() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        prompt(&mut kernel, "执行一个长任务");
        kernel.tick().unwrap();
        kernel.dispatch(WorkspaceAction::Cancel).unwrap();
        let messages = kernel.state.messages.last().unwrap().text.clone();
        assert!(kernel.tick().unwrap().is_none());
        assert_eq!(messages, kernel.state.messages.last().unwrap().text);
        prompt(&mut kernel, "模拟失败");
        drain(&mut kernel);
        assert_eq!(kernel.state.run_status, RunStatus::Failed);
        prompt(&mut kernel, "执行一个长任务");
        drop(kernel);
        let mut kernel = fixture.open();
        assert_eq!(kernel.state.run_status, RunStatus::Cancelled);
        prompt(&mut kernel, "hello");
        drain(&mut kernel);
        assert_eq!(kernel.state.run_status, RunStatus::Completed);
    }
    #[test]
    fn cancelled_proposal_cannot_be_replayed() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        prompt(&mut kernel, "create planner");
        drain(&mut kernel);
        let id = kernel.state.approvals[0].id.clone();
        kernel.dispatch(WorkspaceAction::Cancel).unwrap();
        assert!(kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: id,
                allow: true
            })
            .is_err());
        assert_eq!(kernel.state.modules.len(), 3);
    }
    #[test]
    fn containment_rejects_traversal_absolute_and_symlink_escape() {
        let fixture = Fixture::new();
        let kernel = fixture.open();
        for path in [
            "../escape.md",
            "/tmp/escape.md",
            "notes/../../escape.md",
            "notes\\escape.md",
        ] {
            assert!(
                contained_path(&kernel.root, path).is_err(),
                "accepted {path}"
            );
        }
        #[cfg(unix)]
        {
            let outside = Fixture::new();
            fs::create_dir_all(&outside.path).unwrap();
            std::os::unix::fs::symlink(&outside.path, fixture.path.join("escape")).unwrap();
            assert!(atomic_write(&kernel.root, "escape/pwn.md", "blocked", None).is_err());
            assert!(!outside.path.join("pwn.md").exists());
            std::os::unix::fs::symlink(
                outside.path.join("missing.md"),
                fixture.path.join("notes/dangling.md"),
            )
            .unwrap();
            assert!(atomic_write(&kernel.root, "notes/dangling.md", "blocked", None).is_err());
        }
    }
    #[cfg(windows)]
    #[test]
    fn windows_root_names_are_rejected_before_directory_creation() {
        let fixture = Fixture::new();
        for path in [
            fixture.path.join("aliased."),
            fixture.path.join("stream:payload"),
            fixture.path.join("NUL.txt"),
        ] {
            assert!(Kernel::open(&path).is_err(), "accepted {}", path.display());
        }
        assert!(!fixture.path.join("aliased").exists());
        for path in [
            "C:relative",
            "\\\\server\\share\\workspace",
            "\\\\.\\C:\\workspace",
        ] {
            assert!(Kernel::open(Path::new(path)).is_err(), "accepted {path}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_nested_absolute_cwd_is_saved_with_portable_separators_and_reopens() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        fs::create_dir_all(kernel.root.join("projects/research")).unwrap();
        let mut agent = kernel.state.agent.clone();
        agent.cwd = kernel
            .root
            .join("projects/research")
            .to_string_lossy()
            .into_owned();
        kernel
            .dispatch(WorkspaceAction::SaveAgent { agent })
            .unwrap();
        assert_eq!(kernel.state.agent.cwd, "projects/research");
        drop(kernel);
        let kernel = fixture.open();
        assert_eq!(kernel.state.agent.cwd, "projects/research");
        assert_eq!(
            resolve_agent_cwd(&kernel.root, &kernel.state.agent.cwd).unwrap(),
            kernel.root.join("projects/research")
        );
    }

    #[test]
    fn audit_is_append_only_and_recent_window_is_bounded() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        for n in 0..215 {
            kernel.audit("test", &n.to_string(), "host").unwrap();
        }
        kernel.persist().unwrap();
        assert_eq!(kernel.state.events.len(), RECENT_EVENTS);
        assert!(kernel.connection.execute("DELETE FROM events", []).is_err());
        assert!(kernel
            .connection
            .execute("UPDATE events SET message='forged'", [])
            .is_err());
        let count: i64 = kernel
            .connection
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert!(count > RECENT_EVENTS as i64);
        drop(kernel);
        let kernel = fixture.open();
        assert_eq!(kernel.state.events.len(), RECENT_EVENTS);
        assert!(kernel.state.events.windows(2).all(|w| w[0].seq < w[1].seq));
    }
    #[test]
    fn descriptor_rejects_secrets_and_never_claims_stdio_connected() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let mut agent = kernel.state.agent.clone();
        agent
            .env
            .insert("OPENAI_API_KEY".into(), "test-not-a-secret".into());
        assert!(kernel
            .dispatch(WorkspaceAction::SaveAgent {
                agent: agent.clone()
            })
            .is_err());
        agent.env.clear();
        agent.transport = "stdio".into();
        agent.command = "/bin/echo".into();
        agent.probe_status = "connected".into();
        kernel
            .dispatch(WorkspaceAction::SaveAgent { agent })
            .unwrap();
        assert!(kernel.dispatch(WorkspaceAction::ProbeAgent).is_err());
        assert_ne!(kernel.state.agent.probe_status, "connected");
        assert_ne!(kernel.state.connection.status, "connected");
        assert!(kernel
            .dispatch(WorkspaceAction::Prompt {
                text: "hi".into(),
                module_id: None
            })
            .is_err());
    }
    fn persist_legacy_cwd(kernel: &mut Kernel, relative: &str) {
        // Simulate the pre-migration on-disk format without going through SaveAgent.
        let directory = kernel.root.join(relative);
        fs::create_dir_all(&directory).unwrap();
        kernel.state.agent.cwd = directory.to_string_lossy().into_owned();
        kernel.persist().unwrap();
    }

    fn copy_directory(source: &Path, target: &Path) {
        fs::create_dir_all(target).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let destination = target.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_directory(&entry.path(), &destination);
            } else {
                fs::copy(entry.path(), destination).unwrap();
            }
        }
    }

    #[test]
    fn saved_agent_cwd_is_relative_and_survives_manifest_only_move() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        assert_eq!(kernel.state.agent.cwd, "");
        let mut root_agent = kernel.state.agent.clone();
        root_agent.cwd = ".".into();
        kernel
            .dispatch(WorkspaceAction::SaveAgent { agent: root_agent })
            .unwrap();
        assert_eq!(kernel.state.agent.cwd, "");
        let directory = kernel.root.join("projects/research");
        fs::create_dir_all(&directory).unwrap();
        let mut agent = kernel.state.agent.clone();
        agent.cwd = directory.to_string_lossy().into_owned();
        kernel
            .dispatch(WorkspaceAction::SaveAgent { agent })
            .unwrap();
        assert_eq!(kernel.state.agent.cwd, "projects/research");
        let manifest: WorkspaceManifest =
            read_json(&kernel.root.join(".workspace/workspace.json")).unwrap();
        assert_eq!(manifest.agent.cwd, "projects/research");
        drop(kernel);
        fs::remove_file(fixture.path.join(".workspace/workspace.sqlite3")).unwrap();
        let moved = Fixture::new();
        fs::rename(&fixture.path, &moved.path).unwrap();
        let kernel = moved.open();
        assert_eq!(kernel.state.agent.cwd, "projects/research");
        assert_eq!(
            resolve_agent_cwd(&kernel.root, &kernel.state.agent.cwd).unwrap(),
            kernel.root.join("projects/research")
        );
    }

    #[test]
    fn moved_legacy_root_and_subdirectory_rebase_with_or_without_manifest() {
        for relative in ["", "projects/research"] {
            for keep_manifest in [true, false] {
                let fixture = Fixture::new();
                let mut kernel = fixture.open();
                persist_legacy_cwd(&mut kernel, relative);
                let previous_root = kernel.root.clone();
                drop(kernel);
                if !keep_manifest {
                    fs::remove_file(fixture.path.join(".workspace/workspace.json")).unwrap();
                }
                let moved = Fixture::new();
                fs::rename(&fixture.path, &moved.path).unwrap();
                assert!(!previous_root.exists());
                let kernel = moved.open();
                let expected = relative;
                assert_eq!(kernel.state.agent.cwd, expected);
                assert_eq!(
                    resolve_agent_cwd(&kernel.root, &kernel.state.agent.cwd).unwrap(),
                    kernel.root.join(relative)
                );
                drop(kernel);
                assert_eq!(moved.open().state.agent.cwd, expected);
            }
        }
    }

    #[test]
    fn copied_legacy_workspace_uses_its_own_directory_while_original_exists() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        persist_legacy_cwd(&mut kernel, "projects/research");
        let previous_root = kernel.root.clone();
        drop(kernel);
        let copied = Fixture::new();
        copy_directory(&fixture.path, &copied.path);
        let kernel = copied.open();
        assert!(previous_root.exists());
        assert_eq!(kernel.state.agent.cwd, "projects/research");
        assert_eq!(
            resolve_agent_cwd(&kernel.root, &kernel.state.agent.cwd).unwrap(),
            kernel.root.join("projects/research")
        );
    }

    #[test]
    fn legacy_workspace_can_move_to_an_ancestor_of_its_previous_root() {
        let fixture = Fixture::new();
        let original = fixture.path.join("original");
        let mut kernel = Kernel::open(&original).unwrap();
        persist_legacy_cwd(&mut kernel, "notes");
        drop(kernel);
        let temporary = Fixture::new();
        fs::rename(&original, &temporary.path).unwrap();
        fs::remove_dir(&fixture.path).unwrap();
        fs::rename(&temporary.path, &fixture.path).unwrap();
        let kernel = fixture.open();
        assert_eq!(kernel.state.agent.cwd, "notes");
        assert_eq!(
            resolve_agent_cwd(&kernel.root, &kernel.state.agent.cwd).unwrap(),
            kernel.root.join("notes")
        );
    }

    #[test]
    fn cwd_normalization_rejects_external_traversal_and_unknown_legacy_roots() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let outside = Fixture::new();
        fs::create_dir_all(&outside.path).unwrap();
        for cwd in [
            outside.path.to_string_lossy().into_owned(),
            "../escape".into(),
            "notes/../../escape".into(),
            "missing-directory".into(),
        ] {
            let mut agent = kernel.state.agent.clone();
            agent.cwd = cwd;
            assert!(kernel
                .dispatch(WorkspaceAction::SaveAgent { agent })
                .is_err());
            assert_eq!(kernel.state.agent.cwd, "");
        }
        // A legacy absolute cwd outside the recorded root must never be rebased.
        kernel.state.agent.cwd = outside.path.to_string_lossy().into_owned();
        kernel.persist().unwrap();
        drop(kernel);
        let moved = Fixture::new();
        fs::rename(&fixture.path, &moved.path).unwrap();
        assert!(Kernel::open(&moved.path).is_err());

        // Without a snapshot there is no evidence of the former root. Do not
        // guess that any absolute directory in the manifest means this root.
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        persist_legacy_cwd(&mut kernel, "");
        drop(kernel);
        fs::remove_file(fixture.path.join(".workspace/workspace.sqlite3")).unwrap();
        let moved = Fixture::new();
        fs::rename(&fixture.path, &moved.path).unwrap();
        assert!(Kernel::open(&moved.path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn migrated_cwd_rechecks_symlink_containment_in_the_new_workspace() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        persist_legacy_cwd(&mut kernel, "projects/research");
        drop(kernel);
        let moved = Fixture::new();
        fs::rename(&fixture.path, &moved.path).unwrap();
        let outside = Fixture::new();
        fs::create_dir_all(&outside.path).unwrap();
        fs::remove_dir(moved.path.join("projects/research")).unwrap();
        std::os::unix::fs::symlink(&outside.path, moved.path.join("projects/research")).unwrap();
        assert!(Kernel::open(&moved.path).is_err());
    }

    #[test]
    fn serde_matches_typescript_and_rejects_forged_approval_payload() {
        let action: WorkspaceAction = serde_json::from_str(
            r#"{"type":"create_module","moduleType":"planner","title":"本周"}"#,
        )
        .unwrap();
        assert!(matches!(
            action,
            WorkspaceAction::CreateModule {
                module_type: ModuleType::Planner,
                ..
            }
        ));
        assert!(serde_json::from_str::<WorkspaceAction>(
            r#"{"type":"decide_approval","approvalId":"id","allow":true,"after":"forged"}"#
        )
        .is_err());
        let fixture = Fixture::new();
        let kernel = fixture.open();
        let value = serde_json::to_value(kernel.snapshot().unwrap()).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["modules"][0]["type"], "conversation");
        assert!(value.get("rootPath").is_some());
        assert!(value["modules"][0].get("filePath").is_some());
    }
    #[test]
    fn interrupted_writes_recover_without_false_success_or_duplicate_application() {
        for point in [
            "before_file_write",
            "after_file_write",
            "after_pending_commit",
            "after_manifests",
        ] {
            let fixture = Fixture::new();
            let mut kernel = fixture.open();
            let original = document(&kernel);
            let after = format!("{}\nUnique approved addition", original.content);
            kernel
                .dispatch(WorkspaceAction::EditDocument {
                    module_id: original.id.clone(),
                    content: after.clone(),
                    revision: original.revision.clone(),
                })
                .unwrap();
            let id = kernel.state.approvals[0].id.clone();
            kernel.failpoint = Some(point);
            assert!(kernel
                .dispatch(WorkspaceAction::DecideApproval {
                    approval_id: id.clone(),
                    allow: true
                })
                .is_err());
            assert_eq!(kernel.state.run_status, RunStatus::Failed);
            assert!(kernel
                .dispatch(WorkspaceAction::DecideApproval {
                    approval_id: id.clone(),
                    allow: true
                })
                .is_err());
            let complete_count: i64 = kernel
                .connection
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE kind='completed'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                complete_count, 0,
                "completion logged before commit at {point}"
            );
            drop(kernel);
            let mut kernel = fixture.open();
            if point == "before_file_write" {
                assert_eq!(document(&kernel).content, original.content);
                assert_eq!(kernel.state.approvals.len(), 1);
                kernel
                    .dispatch(WorkspaceAction::DecideApproval {
                        approval_id: id.clone(),
                        allow: true,
                    })
                    .unwrap();
            } else {
                assert!(kernel.state.approvals.is_empty());
                assert!(kernel
                    .dispatch(WorkspaceAction::DecideApproval {
                        approval_id: id.clone(),
                        allow: true
                    })
                    .is_err());
            }
            assert_eq!(document(&kernel).content, after);
            let audit_count: i64 = kernel
                .connection
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE kind='storage/recovered'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            drop(kernel);
            let kernel = fixture.open();
            assert_eq!(document(&kernel).content, after);
            assert_eq!(
                audit_count,
                kernel
                    .connection
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM events WHERE kind='storage/recovered'",
                        [],
                        |r| r.get(0)
                    )
                    .unwrap()
            );
        }
    }

    #[test]
    fn pending_write_recovery_preserves_external_conflict_and_unapplied_proposal() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let doc = document(&kernel);
        kernel
            .dispatch(WorkspaceAction::EditDocument {
                module_id: doc.id.clone(),
                content: "approved".into(),
                revision: doc.revision,
            })
            .unwrap();
        let id = kernel.state.approvals[0].id.clone();
        kernel.failpoint = Some("after_file_write");
        assert!(kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: id.clone(),
                allow: true
            })
            .is_err());
        drop(kernel);
        fs::write(
            fixture.path.join(doc.file_path.unwrap()),
            "external after interrupted write",
        )
        .unwrap();
        let mut kernel = fixture.open();
        assert_eq!(
            document(&kernel).content,
            "external after interrupted write"
        );
        assert_eq!(kernel.state.approvals.len(), 1);
        assert!(kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: id,
                allow: true
            })
            .is_err());
        assert_eq!(
            document(&kernel).content,
            "external after interrupted write"
        );
    }

    #[test]
    fn pending_manifest_commit_restores_layout_and_history_without_reapplying_actions() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let id = kernel.state.modules[0].id.clone();
        kernel.failpoint = Some("after_pending_commit");
        assert!(kernel
            .dispatch(WorkspaceAction::SetLayouts {
                layouts: vec![LayoutChange {
                    id: id.clone(),
                    layout: Layout {
                        x: 0,
                        y: 222,
                        w: 12,
                        h: 43
                    }
                }]
            })
            .is_err());
        drop(kernel);
        let kernel = fixture.open();
        assert_eq!(kernel.module(&id).unwrap().layout.y, 222);
        assert_eq!(
            kernel
                .state
                .events
                .iter()
                .filter(|e| e.kind == "layout/updated")
                .count(),
            1
        );
        drop(kernel);
        let kernel = fixture.open();
        assert_eq!(
            kernel
                .state
                .events
                .iter()
                .filter(|e| e.kind == "layout/updated")
                .count(),
            1
        );
    }

    #[test]
    fn document_creation_recovers_the_same_id_after_file_commit() {
        for agent_created in [false, true] {
            let fixture = Fixture::new();
            let mut kernel = fixture.open();
            let original_count = kernel.state.modules.len();
            let approval_id = if agent_created {
                prompt(&mut kernel, "create document");
                drain(&mut kernel);
                Some(kernel.state.approvals[0].id.clone())
            } else {
                None
            };
            kernel.failpoint = Some("after_create_file");
            let result = if let Some(id) = &approval_id {
                kernel.dispatch(WorkspaceAction::DecideApproval {
                    approval_id: id.clone(),
                    allow: true,
                })
            } else {
                kernel.dispatch(WorkspaceAction::CreateModule {
                    module_type: ModuleType::Document,
                    title: "one document".into(),
                })
            };
            assert!(result.is_err());
            let expected_id: String = kernel
                .connection
                .query_row(
                    "SELECT id FROM create_operations WHERE status='pending'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let files_before = fs::read_dir(fixture.path.join("notes")).unwrap().count();
            drop(kernel);
            let mut kernel = fixture.open();
            assert_eq!(kernel.state.modules.len(), original_count + 1);
            assert!(kernel.module(&expected_id).is_ok());
            if let Some(id) = approval_id {
                assert!(kernel
                    .dispatch(WorkspaceAction::DecideApproval {
                        approval_id: id,
                        allow: true
                    })
                    .is_err());
            }
            drop(kernel);
            let kernel = fixture.open();
            assert_eq!(kernel.state.modules.len(), original_count + 1);
            assert_eq!(
                fs::read_dir(fixture.path.join("notes")).unwrap().count(),
                files_before
            );
        }
    }

    #[test]
    fn connector_mapping_is_durable_before_text_and_multi_turn_reuses_provider_session() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let workspace_id = kernel.state.workspace_id.clone();
        prompt(&mut kernel, "你好");
        kernel.tick().unwrap();
        let provider = kernel.state.provider_session_id.clone().unwrap();
        let runtime = kernel.state.session_id.clone().unwrap();
        let mapping: String = kernel
            .connection
            .query_row(
                "SELECT provider_session_id FROM provider_sessions WHERE runtime_session_id=?1",
                [&runtime],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mapping, provider);
        let events = &kernel.state.events;
        let new = events
            .iter()
            .position(|event| event.kind == "session/new")
            .unwrap();
        let text = events
            .iter()
            .position(|event| event.kind == "session/update")
            .unwrap();
        assert!(new < text);
        assert_eq!(
            events[new].context.as_ref().unwrap().workspace_id,
            workspace_id
        );
        assert!(events[new].connector_seq.unwrap() < events[text].connector_seq.unwrap());
        drain(&mut kernel);
        prompt(&mut kernel, "再聊一轮");
        drain(&mut kernel);
        assert_eq!(kernel.state.session_id.as_deref(), Some(runtime.as_str()));
        assert_eq!(
            kernel.state.provider_session_id.as_deref(),
            Some(provider.as_str())
        );
        assert_eq!(
            kernel
                .connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM provider_sessions", [], |row| row
                    .get(0))
                .unwrap(),
            1
        );
        drop(kernel);
        let mut kernel = fixture.open();
        assert_eq!(kernel.state.workspace_id, workspace_id);
        prompt(&mut kernel, "重开后新会话");
        kernel.tick().unwrap();
        assert_ne!(kernel.state.session_id.as_deref(), Some(runtime.as_str()));
    }

    struct EventFixture(Vec<ConnectorEvent>);
    impl AgentConnector for EventFixture {
        fn initialize(&mut self, _: &ConnectorContext) -> Result<(), String> {
            Ok(())
        }
        fn new_session(&mut self, _: &ConnectorContext) -> Result<(), String> {
            Ok(())
        }
        fn confirm_session_persisted(
            &mut self,
            _: &ConnectorContext,
            _: &str,
        ) -> Result<(), String> {
            Ok(())
        }
        fn prompt(&mut self, _: ConnectorPrompt) -> Result<(), String> {
            Ok(())
        }
        fn cancel(&mut self, _: &ConnectorContext) -> Result<(), String> {
            Ok(())
        }
        fn dispose(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn poll_events(&mut self) -> Result<Vec<ConnectorEvent>, String> {
            Ok(std::mem::take(&mut self.0))
        }
        fn permission_reply(
            &mut self,
            _: &ConnectorContext,
            _: &serde_json::Value,
            _: Option<&str>,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn connector_late_events_duplicate_sequences_and_tool_notifications_cannot_apply_actions() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        prompt(&mut kernel, "你好");
        let context = kernel.active.as_ref().unwrap().context.clone();
        let mut invalid = vec![context.clone(); 7];
        invalid[0].workspace_id = new_id();
        invalid[1].root.push_str("-another");
        invalid[2].generation = new_id();
        invalid[3].permission_epoch += 1;
        invalid[4].runtime_session_id = new_id();
        invalid[5].run_id = new_id();
        invalid[6].module_id = Some(new_id());
        let mut events: Vec<_> = invalid
            .into_iter()
            .enumerate()
            .map(|(index, context)| ConnectorEvent {
                context,
                seq: index as u64 + 1,
                kind: ConnectorEventKind::ToolProposal {
                    proposal: WorkspaceToolIntent::ModuleProposal {
                        module_type: ModuleType::Document,
                        title: "stale must never apply".into(),
                        content: Some("stale".into()),
                    },
                },
            })
            .collect();
        for (seq, kind) in [
            (
                10,
                ConnectorEventKind::ToolStatus {
                    tool_call_id: "notice".into(),
                    title: "write a file".into(),
                    status: "completed".into(),
                },
            ),
            (
                11,
                ConnectorEventKind::TextDelta {
                    text: "once".into(),
                },
            ),
            (
                11,
                ConnectorEventKind::TextDelta {
                    text: "duplicate".into(),
                },
            ),
            (
                9,
                ConnectorEventKind::TextDelta {
                    text: "out of order".into(),
                },
            ),
            (
                12,
                ConnectorEventKind::Completed {
                    stop_reason: "end_turn".into(),
                },
            ),
        ] {
            events.push(ConnectorEvent {
                context: context.clone(),
                seq,
                kind,
            });
        }
        kernel.connector = Box::new(EventFixture(events));
        kernel.tick().unwrap();
        assert_eq!(kernel.state.modules.len(), 3);
        assert!(kernel.state.approvals.is_empty());
        assert_eq!(kernel.state.messages.last().unwrap().text, "once");
        assert_eq!(kernel.state.run_status, RunStatus::Completed);
        assert!(kernel
            .state
            .events
            .iter()
            .any(|event| event.kind == "tool/status"));
        kernel.connector = Box::new(EventFixture(vec![ConnectorEvent {
            context,
            seq: 13,
            kind: ConnectorEventKind::TextDelta {
                text: "late".into(),
            },
        }]));
        assert!(kernel.tick().unwrap().is_none());
        assert_eq!(kernel.state.messages.last().unwrap().text, "once");
    }

    #[test]
    fn mock_write_proposal_preserves_an_external_edit_during_streaming() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let doc = document(&kernel);
        prompt(&mut kernel, "修改文档");
        fs::write(
            fixture.path.join(doc.file_path.unwrap()),
            "external while agent was streaming",
        )
        .unwrap();
        drain(&mut kernel);
        assert_eq!(kernel.state.run_status, RunStatus::Failed);
        assert!(kernel.state.approvals.is_empty());
        assert_eq!(
            document(&kernel).content,
            "external while agent was streaming"
        );
    }

    #[test]
    fn failed_session_mapping_commit_stops_output_and_recovers_without_replaying_prompt() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        prompt(&mut kernel, "你好");
        let original_generation = kernel.state.workspace_generation.clone();
        kernel.failpoint = Some("after_pending_commit");
        assert!(kernel.tick().is_err());
        assert!(kernel.state.messages.last().unwrap().text.is_empty());
        assert!(kernel.tick().is_err());
        drop(kernel);
        let kernel = fixture.open();
        assert_eq!(kernel.state.run_status, RunStatus::Cancelled);
        let saved_generation: String = kernel
            .connection
            .query_row("SELECT generation FROM provider_sessions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(saved_generation, original_generation);
        assert_ne!(kernel.state.workspace_generation, original_generation);
        assert_eq!(
            kernel
                .connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM provider_sessions", [], |row| row
                    .get(0))
                .unwrap(),
            1
        );
    }

    fn configured_policy(mode: WorkspacePolicy, epoch: u64) -> PolicySnapshot {
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

    #[test]
    fn reopened_policy_restores_epoch_before_pending_approval_and_new_run() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let policy = configured_policy(WorkspacePolicy::Ask, 7);
        kernel.apply_policy(policy.clone()).unwrap();
        prompt(&mut kernel, "create planner");
        drain(&mut kernel);
        let id = kernel.state.approvals[0].id.clone();
        drop(kernel);
        let mut kernel = fixture.open();
        kernel.apply_policy(policy).unwrap();
        assert_eq!(kernel.permission_epoch, 7);
        kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: id,
                allow: true,
            })
            .unwrap();
        prompt(&mut kernel, "hello");
        assert_eq!(kernel.active.as_ref().unwrap().context.permission_epoch, 7);
    }

    #[test]
    fn agent_named_user_cannot_forge_manual_approval_origin() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let mut agent = kernel.state.agent.clone();
        agent.id = "user".into();
        kernel
            .dispatch(WorkspaceAction::SaveAgent { agent })
            .unwrap();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Restricted, 9))
            .unwrap();
        prompt(&mut kernel, "modify document");
        drain(&mut kernel);
        let approval = kernel.state.approvals[0].clone();
        assert_eq!(approval.origin, "agent");
        let before = document(&kernel).content;
        assert!(kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: approval.id,
                allow: true
            })
            .is_err());
        assert_eq!(document(&kernel).content, before);
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Disabled, 10))
            .unwrap();
        assert!(kernel.state.approvals.is_empty());
    }

    #[test]
    fn disabled_agent_policy_keeps_manual_document_and_layout_actions_available() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Disabled, 1))
            .unwrap();
        assert!(kernel
            .dispatch(WorkspaceAction::Prompt {
                text: "hello".into(),
                module_id: None
            })
            .is_err());
        kernel
            .dispatch(WorkspaceAction::CreateModule {
                module_type: ModuleType::Document,
                title: "manual".into(),
            })
            .unwrap();
        let doc = document(&kernel);
        kernel
            .dispatch(WorkspaceAction::EditDocument {
                module_id: doc.id.clone(),
                content: "# User owns this".into(),
                revision: doc.revision,
            })
            .unwrap();
        let approval = kernel.state.approvals[0].clone();
        assert_eq!(approval.origin, "user");
        kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: approval.id,
                allow: true,
            })
            .unwrap();
        assert_eq!(document(&kernel).content, "# User owns this");
        kernel
            .dispatch(WorkspaceAction::SetLayouts {
                layouts: vec![LayoutChange {
                    id: doc.id,
                    layout: Layout {
                        x: 0,
                        y: 333,
                        w: 12,
                        h: 43,
                    },
                }],
            })
            .unwrap();
    }

    #[test]
    fn every_effective_policy_change_cancels_runs_and_invalidates_agent_proposals() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Ask, 1))
            .unwrap();
        prompt(&mut kernel, "长任务");
        let old = kernel.active.as_ref().unwrap().context.clone();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Full, 2))
            .unwrap();
        assert!(kernel.active.is_none());
        assert_eq!(kernel.state.run_status, RunStatus::Cancelled);
        let late = ConnectorEvent {
            context: old,
            seq: 999,
            kind: ConnectorEventKind::TextDelta {
                text: "late".into(),
            },
        };
        assert!(!kernel.accept_connector_event(&late));
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Ask, 3))
            .unwrap();
        prompt(&mut kernel, "create planner");
        drain(&mut kernel);
        let id = kernel.state.approvals[0].id.clone();
        let count = kernel.state.modules.len();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Full, 4))
            .unwrap();
        assert!(kernel.state.approvals.is_empty());
        assert_eq!(kernel.state.modules.len(), count);
        assert!(kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: id,
                allow: true
            })
            .is_err());
        prompt(&mut kernel, "create planner");
        drain(&mut kernel);
        assert!(kernel.state.approvals.is_empty());
        assert_eq!(kernel.state.modules.len(), count + 1);
    }

    #[test]
    fn restrictive_mode_allows_proposals_but_not_agent_application_and_keeps_user_draft() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Restricted, 1))
            .unwrap();
        prompt(&mut kernel, "modify document");
        drain(&mut kernel);
        let p = kernel.state.approvals[0].clone();
        let before = document(&kernel).content;
        assert_eq!(p.origin, "agent");
        assert!(kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: p.id,
                allow: true
            })
            .is_err());
        assert_eq!(document(&kernel).content, before);
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Disabled, 2))
            .unwrap();
        assert!(kernel.state.approvals.is_empty());
        let doc = document(&kernel);
        kernel
            .dispatch(WorkspaceAction::EditDocument {
                module_id: doc.id,
                revision: doc.revision,
                content: "manual draft".into(),
            })
            .unwrap();
        let id = kernel.state.approvals[0].id.clone();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Ask, 3))
            .unwrap();
        assert_eq!(kernel.state.approvals[0].id, id);
        kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: id,
                allow: true,
            })
            .unwrap();
        assert_eq!(document(&kernel).content, "manual draft");
    }

    #[test]
    fn hermes_restricted_and_unconfirmed_ask_cannot_start_a_process() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        let mut agent = kernel.state.agent.clone();
        agent.transport = "stdio".into();
        agent.command = "hermes".into();
        agent.args = vec!["acp".into()];
        kernel
            .dispatch(WorkspaceAction::SaveAgent { agent })
            .unwrap();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Restricted, 1))
            .unwrap();
        assert!(kernel
            .dispatch(WorkspaceAction::ConnectAgent)
            .unwrap_err()
            .contains("只读"));
        assert!(kernel.connector.managed_process_id().is_none());
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Ask, 2))
            .unwrap();
        assert!(kernel
            .dispatch(WorkspaceAction::ConnectAgent)
            .unwrap_err()
            .contains("确认"));
        assert!(kernel.connector.managed_process_id().is_none());
    }

    #[test]
    #[ignore = "requires explicit local Hermes authorization and isolated PIXEL_HERMES_TEST_ROOT"]
    fn real_hermes_authorized_smoke() {
        assert_eq!(std::env::var("PIXEL_REAL_HERMES_RUN").as_deref(), Ok("1"));
        let path = PathBuf::from(
            std::env::var("PIXEL_HERMES_TEST_ROOT").expect("isolated test root required"),
        );
        assert!(path.is_absolute());
        assert!(
            !path.exists(),
            "Real smoke must use a fresh isolated directory"
        );
        let mut kernel = Kernel::open(&path).unwrap();
        let mut descriptor = kernel.state.agent.clone();
        descriptor.id = "hermes".into();
        descriptor.name = "Hermes".into();
        descriptor.transport = "stdio".into();
        descriptor.command = std::env::var("PIXEL_HERMES_COMMAND").unwrap_or("hermes".into());
        descriptor.args = vec!["acp".into()];
        kernel
            .dispatch(WorkspaceAction::SaveAgent { agent: descriptor })
            .unwrap();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Full, 1))
            .unwrap();
        kernel.dispatch(WorkspaceAction::ConnectAgent).unwrap();
        fn until(
            kernel: &mut Kernel,
            deadline: std::time::Duration,
            ready: impl Fn(&Kernel) -> bool,
        ) {
            let start = std::time::Instant::now();
            loop {
                kernel.tick().unwrap();
                if ready(kernel) {
                    break;
                }
                if kernel.state.run_status == RunStatus::Failed
                    || matches!(
                        kernel.state.connection.status.as_str(),
                        "error" | "not_authenticated" | "not_installed"
                    )
                {
                    panic!("Hermes failed: {}", kernel.state.connection.message);
                }
                assert!(start.elapsed() < deadline, "Hermes smoke timed out");
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
        }
        until(&mut kernel, std::time::Duration::from_secs(100), |k| {
            k.state.connection.status == "connected"
        });
        let provider = kernel.state.provider_session_id.clone().unwrap();
        let runtime = kernel.state.session_id.clone().unwrap();
        let pid = kernel.connector.managed_process_id().unwrap();
        println!(
            "REAL_HERMES_CONNECTED version={:?} pid={} runtime={} provider={} caps={}",
            kernel.state.connection.hermes_version,
            pid,
            runtime,
            provider,
            kernel.state.connection.capabilities
        );
        let nonce = format!("pixel-{}", Uuid::new_v4());
        let first=format!("This is an isolated integration test. Do not use any tools or read files. Remember this exact token for my next message: {nonce}. Reply with only the result of 37 multiplied by 19.");
        kernel
            .dispatch(WorkspaceAction::Prompt {
                text: first,
                module_id: None,
            })
            .unwrap();
        until(&mut kernel, std::time::Duration::from_secs(180), |k| {
            k.state.run_status == RunStatus::Completed
        });
        let answer1 = kernel.state.messages.last().unwrap().text.clone();
        assert!(
            answer1.contains("703"),
            "Unexpected real model answer: {answer1}"
        );
        kernel.dispatch(WorkspaceAction::Prompt{text:"Without using tools or files, repeat the exact token I asked you to remember in my previous message. Reply only with the token.".into(),module_id:None}).unwrap();
        until(&mut kernel, std::time::Duration::from_secs(180), |k| {
            k.state.run_status == RunStatus::Completed
        });
        let answer2 = kernel.state.messages.last().unwrap().text.clone();
        assert!(
            answer2.contains(&nonce),
            "Real same-session recall failed: {answer2}"
        );
        assert_eq!(kernel.state.provider_session_id.as_ref(), Some(&provider));
        println!(
            "REAL_HERMES_TWO_TURNS first={} second={} same_provider=true",
            answer1, answer2
        );
        kernel.dispatch(WorkspaceAction::Prompt{text:"Do not use any tools or files. Write a long numbered essay explaining 200 practical ways to organize a home library, each in a detailed paragraph.".into(),module_id:None}).unwrap();
        for _ in 0..20 {
            kernel.tick().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        kernel.dispatch(WorkspaceAction::Cancel).unwrap();
        assert_eq!(kernel.state.run_status, RunStatus::Cancelled);
        assert!(kernel.connector.managed_process_id().is_none());
        println!(
            "REAL_HERMES_CANCELLED original_pid={} process_reaped=true",
            pid
        );
        kernel.dispatch(WorkspaceAction::ConnectAgent).unwrap();
        until(&mut kernel, std::time::Duration::from_secs(100), |k| {
            k.state.connection.status == "connected"
        });
        assert_ne!(kernel.state.provider_session_id.as_ref(), Some(&provider));
        kernel
            .dispatch(WorkspaceAction::Prompt {
                text: "Do not use tools. Reply only with the value of 11 plus 23.".into(),
                module_id: None,
            })
            .unwrap();
        until(&mut kernel, std::time::Duration::from_secs(180), |k| {
            k.state.run_status == RunStatus::Completed
        });
        assert!(kernel.state.messages.last().unwrap().text.contains("34"));
        kernel
            .dispatch(WorkspaceAction::Prompt {
                text: "Do not use tools. Write a very long explanation of 100 prime numbers."
                    .into(),
                module_id: None,
            })
            .unwrap();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Disabled, 2))
            .unwrap();
        assert!(kernel.connector.managed_process_id().is_none());
        assert!(kernel
            .dispatch(WorkspaceAction::Prompt {
                text: "should not run".into(),
                module_id: None
            })
            .is_err());
        println!("REAL_HERMES_REVOKED process_reaped=true blocked_next_prompt=true");
        kernel.shutdown().unwrap();
        let report = serde_json::json!({"hermesVersion":kernel.state.connection.hermes_version,"firstProviderSessionId":provider,"firstRuntimeSessionId":runtime,"originalManagedPid":pid,"firstAnswer":answer1,"secondAnswer":answer2,"nonce":nonce,"cancelReaped":true,"revocationBlocked":true,"scope":"isolated synthetic prompts only; no user documents sent"});
        fs::write(
            path.join("smoke-result.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }

    #[test]
    #[ignore = "explicit real provider authorization, isolated root and bundled MCP binary required"]
    fn real_provider_module_tools_smoke() {
        use std::sync::{Arc, Mutex};
        assert_eq!(std::env::var("ATRIO_REAL_PROVIDER_RUN").as_deref(), Ok("1"));
        let provider = std::env::var("ATRIO_TEST_PROVIDER").unwrap();
        assert!(["hermes", "claude_code", "codex"].contains(&provider.as_str()));
        let path = PathBuf::from(std::env::var("ATRIO_TEST_ROOT").unwrap());
        assert!(
            path.is_absolute() && !path.exists(),
            "fresh isolated workspace required"
        );
        let executable = PathBuf::from(std::env::var("ATRIO_MCP_BINARY").unwrap());
        assert!(executable.is_file());
        let mut kernel = Kernel::open(&path).unwrap();
        let document_id = kernel
            .state
            .modules
            .iter()
            .find(|m| m.module_type == ModuleType::Document)
            .unwrap()
            .id
            .clone();
        let nonce = format!("AT-{}", new_id());
        let doc = kernel.module(&document_id).unwrap().clone();
        let content = format!(
            "# Observatory brief\n\nField token: {nonce}\nTarget: photograph the amber comet.\n"
        );
        kernel
            .dispatch(WorkspaceAction::EditDocument {
                module_id: doc.id,
                content: content.clone(),
                revision: doc.revision,
            })
            .unwrap();
        let approval = kernel.state.approvals[0].id.clone();
        kernel
            .dispatch(WorkspaceAction::DecideApproval {
                approval_id: approval,
                allow: true,
            })
            .unwrap();
        kernel
            .dispatch(WorkspaceAction::CreateModule {
                module_type: ModuleType::Planner,
                title: "Field plan".into(),
            })
            .unwrap();
        let planner_id = kernel.state.modules.last().unwrap().id.clone();
        let dashboard_id = kernel
            .state
            .modules
            .iter()
            .find(|m| m.module_type == ModuleType::Dashboard)
            .unwrap()
            .id
            .clone();
        let mut agent = kernel.state.agent.clone();
        agent.provider = Some(provider.clone());
        agent.id = provider.clone();
        agent.name = provider.clone();
        agent.transport = "stdio".into();
        let spec = NativeProvider::parse(&provider).unwrap();
        agent.command = std::env::var("ATRIO_TEST_COMMAND").unwrap_or(spec.command().into());
        agent.args = spec.args();
        kernel
            .dispatch(WorkspaceAction::SaveAgent { agent })
            .unwrap();
        kernel
            .apply_policy(configured_policy(WorkspacePolicy::Full, 1))
            .unwrap();
        let host = Arc::new(Mutex::new(kernel));
        let serving = host.clone();
        let server = crate::mcp_bridge::ToolServer::start(Arc::new(move |request| {
            serving.lock().unwrap().handle_module_tool_request(
                &request.session_token,
                &request.run_scope,
                &request.tool,
                request.args,
                &request.request_id,
            )
        }))
        .unwrap();
        host.lock()
            .unwrap()
            .configure_tool_bridge(server.path().to_owned(), executable);
        host.lock()
            .unwrap()
            .dispatch(WorkspaceAction::ConnectAgent)
            .unwrap();
        fn wait(host: &Arc<Mutex<Kernel>>, connected: bool) {
            let start = std::time::Instant::now();
            loop {
                let mut k = host.lock().unwrap();
                k.tick().unwrap();
                if connected && k.state.connection.status == "connected" {
                    break;
                }
                if !connected
                    && matches!(
                        k.state.run_status,
                        RunStatus::Completed | RunStatus::WaitingApproval
                    )
                    && k.active.is_none()
                {
                    break;
                }
                if k.state.run_status == RunStatus::Failed
                    || matches!(
                        k.state.connection.status.as_str(),
                        "error" | "not_authenticated" | "not_installed"
                    )
                {
                    panic!("Actual provider failed: {}", k.state.connection.message);
                }
                assert!(
                    start.elapsed() < std::time::Duration::from_secs(240),
                    "real tool turn timeout"
                );
                drop(k);
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
        }
        wait(&host, true);
        let text="Use only the Atrio workspace MCP tools. Read the selected saved Document (not the file system), and report its exact field token and target. The answer is not in this prompt. Do not propose any edits in this turn.";
        host.lock()
            .unwrap()
            .dispatch(WorkspaceAction::Prompt {
                text: text.into(),
                module_id: Some(document_id.clone()),
            })
            .unwrap();
        wait(&host, false);
        {
            let k = host.lock().unwrap();
            assert!(
                k.state.messages.last().unwrap().text.contains(&nonce),
                "live read did not return stored token"
            );
            assert!(k
                .state
                .events
                .iter()
                .any(|e| e.kind == "workspace/tool_read"));
            println!(
                "REAL_MODULE_READ provider={} nonce={} session={:?}",
                provider, nonce, k.state.provider_session_id
            );
        }
        let prompts=[
            (document_id.clone(),"Read this saved Document via workspace tools, then submit a document change that preserves the field token and appends a new section titled Verification with one sentence: Telescope calibrated. Only use Host workspace tools, never filesystem tools.".to_owned()),
            (planner_id.clone(),"Read this Planner via workspace tools. Replace its tasks with exactly three customized tasks: Inspect the amber telescope (done false,time 08:00,tag setup); Record comet coordinates (done true,time 09:00,tag data); Review images (done false,time 10:00,tag review). Use workspace_propose_changes and respect actual tool results.".to_owned()),
            (planner_id.clone(),"Read the same existing Planner again. Preserve its tasks and IDs; change the first task title to Inspect the sapphire telescope and set the third task done=true. Propose this real update using Host tools.".to_owned()),
            (dashboard_id.clone(),format!("Read this Dashboard and the workspace modules. Set its configuration to metrics tasks_done and tasks_total, filtered to plannerIds [\"{planner_id}\"], title Observatory progress. Use Host proposal tools, not arbitrary files.")),
            (planner_id.clone(),"Read this Planner, then propose renaming its module title to Comet observation schedule. Keep its layout and content unchanged. Use Host proposal tools.".to_owned()),
        ];
        for (module_id, text) in prompts {
            let before = host.lock().unwrap().state.module_proposals.len();
            host.lock()
                .unwrap()
                .dispatch(WorkspaceAction::Prompt {
                    text,
                    module_id: Some(module_id),
                })
                .unwrap();
            wait(&host, false);
            let k = host.lock().unwrap();
            assert!(
                k.state.module_proposals.len() > before,
                "Provider did not issue real proposal"
            );
            assert_eq!(k.state.module_proposals.last().unwrap().status, "applied");
        }
        let mut k = host.lock().unwrap();
        let doc = k
            .snapshot()
            .unwrap()
            .modules
            .into_iter()
            .find(|m| m.id == document_id)
            .unwrap();
        assert!(doc.content.contains(&nonce) && doc.content.contains("Telescope calibrated"));
        let plan = k.module(&planner_id).unwrap();
        assert_eq!(plan.tasks.len(), 3);
        assert!(plan.tasks[0].title.contains("sapphire"));
        assert!(plan.tasks[2].done);
        assert_eq!(plan.title, "Comet observation schedule");
        assert_eq!(
            k.module(&dashboard_id)
                .unwrap()
                .dashboard_config
                .as_ref()
                .unwrap()
                .planner_ids,
            vec![planner_id.clone()]
        );
        let report = serde_json::json!({"provider":provider,"readNonce":nonce,"providerSessionId":k.state.provider_session_id,"modules":k.snapshot().unwrap().modules,"proposals":k.state.module_proposals,"events":k.state.events});
        fs::write(
            path.join("real-module-result.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        k.shutdown().unwrap();
        println!(
            "REAL_MODULE_EDITS provider={} document/planner/update/dashboard/title PASS",
            provider
        );
    }

    #[test]
    fn streamed_scope_echo_is_redacted_across_chunks_and_cannot_be_written_into_modules() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        prompt(&mut kernel, "长任务");
        let scope = kernel.module_tool_run_scope().unwrap().to_owned();
        let context = kernel.active.as_ref().unwrap().context.clone();
        for (seq, text) in [(500, scope[..12].to_owned()), (501, scope[12..].to_owned())] {
            kernel
                .consume_connector_event(ConnectorEvent {
                    context: context.clone(),
                    seq,
                    kind: ConnectorEventKind::TextDelta { text },
                })
                .unwrap();
        }
        kernel.persist().unwrap();
        let saved: String = kernel
            .connection
            .query_row("SELECT value FROM metadata WHERE key='snapshot'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!saved.contains(&scope));
        assert!(saved.contains("run-scope-redacted"));
        let token = kernel.module_tool_session_token().unwrap().to_owned();
        assert!(kernel.handle_module_tool(&token,&scope,"workspace_propose_changes",serde_json::json!({"runScope":scope,"requestId":"echo","idempotencyKey":"echo","expectedRevision":"unused","change":{"type":"document","moduleId":document(&kernel).id,"content":scope}})).is_err());
    }

    #[test]
    fn duplicate_dashboard_preserves_selected_metrics_and_filters() {
        let fixture = Fixture::new();
        let mut kernel = fixture.open();
        kernel
            .dispatch(WorkspaceAction::CreateModule {
                module_type: ModuleType::Planner,
                title: "Plan".into(),
            })
            .unwrap();
        let planner = kernel.state.modules.last().unwrap().id.clone();
        let dashboard = kernel
            .state
            .modules
            .iter()
            .find(|m| m.module_type == ModuleType::Dashboard)
            .unwrap()
            .id
            .clone();
        let config = DashboardConfig {
            metrics: vec![module_tools::DashboardMetric::TasksDone],
            planner_ids: vec![planner],
            title: Some("Filtered".into()),
        };
        kernel.module_mut(&dashboard).unwrap().dashboard_config = Some(config.clone());
        kernel.persist().unwrap();
        kernel
            .dispatch(WorkspaceAction::DuplicateModule {
                module_id: dashboard,
            })
            .unwrap();
        assert_eq!(
            kernel
                .state
                .modules
                .last()
                .unwrap()
                .dashboard_config
                .as_ref(),
            Some(&config)
        );
    }

    #[test]
    fn all_action_dtos_roundtrip_the_shared_typescript_contract() {
        use serde_json::json;
        let fixture = Fixture::new();
        let kernel = fixture.open();
        let id = kernel.state.modules[0].id.clone();
        let actions = vec![
            json!({"type":"create_module","moduleType":"planner","title":"本周"}),
            json!({"type":"rename_module","moduleId":id,"title":"新标题"}),
            json!({"type":"close_module","moduleId":id}),
            json!({"type":"duplicate_module","moduleId":id}),
            json!({"type":"set_layouts","layouts":[{"id":id,"layout":{"x":0,"y":50,"w":12,"h":43}}]}),
            json!({"type":"toggle_task","moduleId":id,"taskId":"task-id"}),
            json!({"type":"add_task","moduleId":id,"title":"新任务"}),
            json!({"type":"edit_document","moduleId":id,"content":"# 文档","revision":null}),
            json!({"type":"decide_approval","approvalId":"approval-id","allow":true}),
            json!({"type":"set_permission_mode","mode":"restricted"}),
            json!({"type":"set_overlap","allow":false}),
            json!({"type":"save_agent","agent":kernel.state.agent}),
            json!({"type":"probe_agent"}),
            json!({"type":"prompt","text":"创建一个本周计划","moduleId":null}),
            json!({"type":"cancel"}),
            json!({"type":"set_workspace_policy","mode":"full"}),
            json!({"type":"set_system_policy","mode":"deny_all"}),
            json!({"type":"confirm_hermes_scope","accepted":true}),
            json!({"type":"confirm_agent_scope","accepted":true}),
            json!({"type":"connect_agent"}),
            json!({"type":"disconnect_agent"}),
            json!({"type":"permission_reply","approvalId":"permission-id","optionId":null}),
        ];
        for value in &actions {
            let tag = value["type"].as_str().unwrap();
            let action: WorkspaceAction = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(
                serde_json::to_value(action).unwrap(),
                *value,
                "Rust action changed camelCase fields for {tag}"
            );
        }
        let snapshot = serde_json::to_value(kernel.snapshot().unwrap()).unwrap();
        let fields = [
            "schemaVersion",
            "name",
            "rootPath",
            "modules",
            "messages",
            "events",
            "approvals",
            "runStatus",
            "sessionId",
            "permissionMode",
            "agent",
            "allowOverlap",
            "updatedAt",
            "workspaceGeneration",
            "workspaceId",
            "providerSessionId",
            "connection",
            "moduleProposals",
        ];
        assert_eq!(snapshot.as_object().unwrap().len(), fields.len());
        for field in fields {
            assert!(
                snapshot.get(field).is_some(),
                "Missing snapshot field {field}"
            );
        }
        let fixture =
            serde_json::to_vec(&json!({"snapshot": snapshot, "actions": actions})).unwrap();
        let mut validator = std::process::Command::new("node")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../scripts/check-rust-contract.mjs"
            ))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Contract validation requires Node.js and npm ci in the project root");
        validator.stdin.take().unwrap().write_all(&fixture).unwrap();
        let validation = validator.wait_with_output().unwrap();
        assert!(
            validation.status.success(),
            "TypeScript contract validation failed:\n{}\n{}",
            String::from_utf8_lossy(&validation.stdout),
            String::from_utf8_lossy(&validation.stderr)
        );
        // Explicit opt-in lets integration QA consume actual Rust-serialized JSON.
        if let Ok(path) = std::env::var("PIXEL_CONTRACT_FIXTURE") {
            fs::write(
                path,
                serde_json::to_string_pretty(&json!({"snapshot": snapshot, "actions": actions}))
                    .unwrap(),
            )
            .unwrap();
        }
    }
}
