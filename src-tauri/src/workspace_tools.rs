//! Deliberately small Host-owned tool surface. A connector may propose these
//! structured intents; only the Host validates them and creates approvals.
use crate::agent::{ConnectorContext, ModuleContext};
use crate::kernel::ModuleType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum WorkspaceToolIntent {
    ModuleProposal {
        module_type: ModuleType,
        title: String,
        content: Option<String>,
    },
    WriteProposal {
        module_id: String,
        title: String,
        before_revision: String,
        content: String,
    },
}

pub trait HostWorkspaceTools {
    fn module_context(&self) -> Result<Vec<ModuleContext>, String>;
    fn propose_tool(
        &mut self,
        context: &ConnectorContext,
        proposal: WorkspaceToolIntent,
    ) -> Result<(), String>;
}

pub struct ToolGateway;
impl ToolGateway {
    pub fn propose(
        host: &mut impl HostWorkspaceTools,
        context: &ConnectorContext,
        proposal: WorkspaceToolIntent,
    ) -> Result<(), String> {
        host.propose_tool(context, proposal)
    }
}
