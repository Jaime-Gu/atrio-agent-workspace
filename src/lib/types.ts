export type ModuleType = "conversation" | "planner" | "document" | "dashboard";
export type ModuleStatus =
  "idle" | "running" | "waiting_approval" | "attention" | "error" | "offline";
export type RunStatus =
  | "idle"
  | "running"
  | "waiting_approval"
  | "completed"
  | "failed"
  | "cancelled";
export interface Layout {
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface Task {
  id: string;
  title: string;
  done: boolean;
  time: string;
  tag: string;
}
export interface Message {
  id: string;
  role: "user" | "assistant";
  text: string;
  timestamp: string;
}
export type AgentProvider = "mock" | "hermes" | "claude_code" | "codex";
export type DashboardMetric =
  "tasks_done" | "tasks_total" | "module_count" | "document_count";
export interface DashboardConfig {
  metrics: DashboardMetric[];
  plannerIds: string[];
  title?: string;
}
export interface ModuleProposal {
  id: string;
  status: "pending" | "applied" | "rejected" | "conflict" | "cancelled";
  title: string;
  moduleId?: string | null;
  resultRevision?: string | null;
  summary: string;
  createdAt?: string;
  updatedAt?: string;
}
export interface WorkspaceModule {
  id: string;
  type: ModuleType;
  title: string;
  status: ModuleStatus;
  layout: Layout;
  tasks: Task[];
  content: string;
  filePath: string | null;
  revision: string | null;
  moduleRevision?: string;
  dashboardConfig?: DashboardConfig | null;
}
export interface AgentDescriptor {
  provider?: AgentProvider;
  id: string;
  name: string;
  transport: "mock" | "stdio";
  command: string;
  args: string[];
  env: Record<string, string>;
  cwd: string;
  probeStatus: string;
}
export interface AgentEvent {
  seq: number;
  kind: string;
  message: string;
  timestamp: string;
  sessionId: string | null;
  actor?: string;
  context?: {
    workspaceId: string;
    root: string;
    generation: string;
    permissionEpoch: number;
    runtimeSessionId: string;
    runId: string;
    moduleId: string | null;
  } | null;
  connectorSeq?: number | null;
}
export interface Approval {
  id: string;
  kind: "create_module" | "write_file" | "agent_permission" | "module_changes";
  title: string;
  description: string;
  moduleId: string | null;
  moduleType: ModuleType | null;
  filePath: string | null;
  before: string | null;
  after: string | null;
  revision: string | null;
  origin?: "user" | "agent";
  epoch?: number;
  options?: { id: string; name: string; kind: string }[];
  scope?: string;
  proposalId?: string;
  changes?: {
    type: "document" | "planner" | "dashboard" | "metadata" | "create";
    [key: string]: unknown;
  };
}
export type PolicyMode = "disabled" | "restricted" | "ask" | "full";
export type SystemPolicy = "workspace" | "allow_all" | "deny_all";
export interface WorkspacePolicy {
  system: SystemPolicy;
  local: PolicyMode;
  effective: PolicyMode;
  source: "workspace" | "system";
  epoch: number;
  workspaceTrusted: boolean;
  hermesScopeAccepted: boolean;
  agentScopeAccepted?: boolean;
  scopeProvider?: AgentProvider | null;
}
export interface AgentConnection {
  probe?: {
    provider: string;
    hostExecutable: string;
    hostVersion: string;
    executable: string;
    version: string;
    hostAvailable: boolean;
    adapterAvailable: boolean;
    acpAvailable: boolean;
    authenticationStatus: string;
    handshakeStatus: string;
    sessionStatus: string;
    restrictedSupported: boolean;
    detail: string;
  } | null;
  provider?: AgentProvider;
  providerVersion?: string | null;
  status:
    | "not_installed"
    | "not_authenticated"
    | "disconnected"
    | "connecting"
    | "connected"
    | "stopping"
    | "error";
  message: string;
  providerSessionId: string | null;
  capabilities: unknown;
  hermesVersion: string | null;
}
export interface WorkspaceSnapshot {
  schemaVersion: 1;
  name: string;
  rootPath: string;
  workspaceId?: string;
  providerSessionId?: string | null;
  // Assigned anew by the Host each time a workspace is opened, never a stored trust ID.
  // Optional only for loading older browser snapshots and read-only fixtures.
  workspaceGeneration?: string;
  modules: WorkspaceModule[];
  messages: Message[];
  events: AgentEvent[];
  approvals: Approval[];
  moduleProposals?: ModuleProposal[];
  runStatus: RunStatus;
  sessionId: string | null;
  permissionMode: "ask" | "restricted";
  policy?: WorkspacePolicy;
  connection?: AgentConnection;
  agent: AgentDescriptor;
  allowOverlap: boolean;
  updatedAt: string;
}
export type WorkspaceAction =
  | { type: "create_module"; moduleType: ModuleType; title: string }
  | { type: "rename_module"; moduleId: string; title: string }
  | { type: "close_module"; moduleId: string }
  | { type: "duplicate_module"; moduleId: string }
  | { type: "set_layouts"; layouts: { id: string; layout: Layout }[] }
  | { type: "toggle_task"; moduleId: string; taskId: string }
  | { type: "add_task"; moduleId: string; title: string }
  | {
      type: "edit_document";
      moduleId: string;
      content: string;
      revision: string | null;
    }
  | { type: "decide_approval"; approvalId: string; allow: boolean }
  | { type: "set_permission_mode"; mode: "ask" | "restricted" }
  | { type: "set_workspace_policy"; mode: PolicyMode }
  | { type: "set_system_policy"; mode: SystemPolicy }
  | { type: "confirm_hermes_scope"; accepted: boolean }
  | { type: "confirm_agent_scope"; accepted: boolean }
  | { type: "connect_agent" }
  | { type: "disconnect_agent" }
  | { type: "permission_reply"; approvalId: string; optionId: string | null }
  | { type: "set_overlap"; allow: boolean }
  | { type: "save_agent"; agent: AgentDescriptor }
  | { type: "probe_agent" }
  | { type: "prompt"; text: string; moduleId: string | null }
  | { type: "cancel" };
