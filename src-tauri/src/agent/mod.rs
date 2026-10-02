//! Connectors own agent protocol/stream mechanics, never Host authorization or disk writes.
pub mod mock;
pub mod native;
pub mod providers;

use crate::kernel::ModuleType;
use crate::workspace_tools::WorkspaceToolIntent;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorContext {
    pub workspace_id: String,
    pub root: String,
    pub generation: String,
    pub permission_epoch: u64,
    pub runtime_session_id: String,
    pub run_id: String,
    pub module_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModuleContext {
    pub id: String,
    pub module_type: ModuleType,
    pub title: String,
    pub content: String,
    pub revision: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectorPrompt {
    pub context: ConnectorContext,
    pub text: String,
    /// Host-provided read snapshot for Mock. Native sends only `text` by default.
    pub modules: Vec<ModuleContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ConnectorEventKind {
    Initialized {
        protocol_version: u64,
        capabilities: Value,
    },
    SessionCreated {
        provider_session_id: String,
    },
    TextDelta {
        text: String,
    },
    /// ACP notifications are presentation/audit only, never a ToolGateway call.
    ToolStatus {
        tool_call_id: String,
        title: String,
        status: String,
    },
    ToolProposal {
        proposal: WorkspaceToolIntent,
    },
    PermissionRequest {
        request_id: Value,
        title: String,
        options: Vec<PermissionOption>,
    },
    Completed {
        stop_reason: String,
    },
    Failed {
        code: String,
        message: String,
    },
    Cancelled,
    Disconnected {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorEvent {
    pub context: ConnectorContext,
    pub seq: u64,
    pub kind: ConnectorEventKind,
}

/// All entry points must return promptly: Native's process and protocol I/O run
/// in a worker. Successful calls mean queued, not a completed handshake or task.
pub trait AgentConnector: Send {
    /// Only the process created and owned by this connector; never name-based.
    fn managed_process_id(&self) -> Option<u32> {
        None
    }
    fn initialize(&mut self, context: &ConnectorContext) -> Result<(), String>;
    fn new_session(&mut self, context: &ConnectorContext) -> Result<(), String>;
    fn confirm_session_persisted(
        &mut self,
        context: &ConnectorContext,
        provider_session_id: &str,
    ) -> Result<(), String>;
    fn prompt(&mut self, prompt: ConnectorPrompt) -> Result<(), String>;
    fn cancel(&mut self, context: &ConnectorContext) -> Result<(), String>;
    fn dispose(&mut self) -> Result<(), String>;
    /// Explicit bounded shutdown barrier. Native overrides this to confirm its
    /// worker-managed process group has exited before Host allows a new run.
    fn shutdown(&mut self) -> Result<(), String> {
        self.dispose()
    }
    fn poll_events(&mut self) -> Result<Vec<ConnectorEvent>, String>;
    fn permission_reply(
        &mut self,
        context: &ConnectorContext,
        request_id: &Value,
        option_id: Option<&str>,
    ) -> Result<(), String>;
}
