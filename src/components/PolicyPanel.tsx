import { useState } from "react";
import { Settings2, ShieldCheck } from "lucide-react";
import type {
  AgentConnection,
  PolicyMode,
  WorkspaceAction,
  WorkspacePolicy,
  WorkspaceSnapshot,
} from "../lib/types";
import { Dialog } from "./Dialog";
import {
  agentProvider,
  providerName,
  providers,
  scopeAccepted,
} from "../lib/providers";

export const policyLabels: Record<PolicyMode, string> = {
  disabled: "禁止运行",
  restricted: "受限 / 只读",
  ask: "询问",
  full: "完全访问",
};
export const connectionLabels: Record<AgentConnection["status"], string> = {
  not_installed: "未找到宿主或适配器",
  not_authenticated: "待认证",
  disconnected: "未连接",
  connecting: "正在连接",
  connected: "已连接",
  stopping: "正在停止",
  error: "连接错误",
};
export function workspacePolicy(snapshot: WorkspaceSnapshot): WorkspacePolicy {
  return (
    snapshot.policy ?? {
      system: "workspace",
      local: snapshot.permissionMode,
      effective: snapshot.permissionMode,
      source: "workspace",
      epoch: 0,
      workspaceTrusted: true,
      hermesScopeAccepted: false,
    }
  );
}
export function policyBlockReason(snapshot: WorkspaceSnapshot): string | null {
  const policy = workspacePolicy(snapshot);
  if (policy.effective === "disabled")
    return "当前权限禁止 Agent 运行；你仍可手动编辑和调整布局。";
  if (snapshot.agent.transport === "stdio") {
    if (policy.effective === "restricted")
      return `${providerName(snapshot.agent)} 的完整只读约束尚未验证，受限模式不能连接或发送任务。`;
    if (policy.effective === "ask" && !scopeAccepted(policy, snapshot.agent))
      return `请先在工作区权限中明确接受 ${providerName(snapshot.agent)} 本次本机运行范围。`;
  }
  return null;
}
export function AgentScope({
  snapshot,
  dispatch,
  busy = false,
}: {
  snapshot: WorkspaceSnapshot;
  dispatch: (action: WorkspaceAction) => Promise<void>;
  busy?: boolean;
}) {
  const policy = workspacePolicy(snapshot);
  const name = providerName(snapshot.agent);
  const provider = agentProvider(snapshot.agent);
  const [saving, setSaving] = useState(false),
    [error, setError] = useState("");
  return (
    <div className="hermes-scope">
      <p>
        {name}{" "}
        是本机进程，起始工作目录不是沙箱。其自有文件、命令和网络工具未由此工作区完整托管。
        {providers[provider].profile}
      </p>
      <p>
        工作区模块工具的写入由 Host 统一校验与审批。{name}{" "}
        自有工具仅在它实际发来权限请求时可审批，不保证每次文件操作都会询问。受限
        / 只读尚未验证，当前禁止该模式连接 {name}。
      </p>
      {policy.effective === "ask" && (
        <label className="scope-confirm">
          <input
            type="checkbox"
            checked={scopeAccepted(policy, snapshot.agent)}
            disabled={busy || saving}
            onChange={async (e) => {
              const accepted = e.target.checked;
              setSaving(true);
              setError("");
              try {
                await dispatch({ type: "confirm_agent_scope", accepted });
              } catch (err) {
                setError(err instanceof Error ? err.message : String(err));
              } finally {
                setSaving(false);
              }
            }}
          />
          <span>
            我接受 {name}{" "}
            在本工作区的本机运行范围，并了解它的自有工具未被完整托管。
          </span>
        </label>
      )}
      {policy.effective === "full" && (
        <p>
          已选择完全访问。经 Host 处理的写入仍校验路径、模块版本和权限；{name}{" "}
          自有工具不受这些 Host 检查完整覆盖。
        </p>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
export function PolicyPanel({
  snapshot,
  dispatch,
  onClose,
  onSettings,
}: {
  snapshot: WorkspaceSnapshot;
  dispatch: (action: WorkspaceAction) => Promise<void>;
  onClose: () => void;
  onSettings: () => void;
}) {
  const policy = workspacePolicy(snapshot);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  return (
    <Dialog title="工作区权限" onClose={onClose}>
      <div className="policy-summary">
        <ShieldCheck size={24} />
        <div>
          <strong>
            {policyLabels[policy.effective]} ·{" "}
            {policy.source === "system" ? "系统" : "工作区"}
          </strong>
          <p>{snapshot.name}</p>
        </div>
      </div>
      <p className="path-text">{snapshot.rootPath}</p>
      {policy.source === "system" && (
        <p className="inline-warning">
          系统策略正在覆盖此工作区。原工作区选择“{policyLabels[policy.local]}
          ”已保留，恢复“按工作区设置”后生效。
        </p>
      )}
      {!policy.workspaceTrusted && (
        <p className="inline-warning">
          这是尚未确认局部权限的工作区，复制或迁移后的工作区不会自动继承旧授权。
        </p>
      )}
      <div className="policy-options" role="group" aria-label="工作区权限模式">
        {(Object.keys(policyLabels) as PolicyMode[]).map((mode) => (
          <button
            key={mode}
            className={policy.local === mode ? "chosen" : ""}
            aria-pressed={policy.local === mode}
            disabled={busy || policy.source === "system"}
            onClick={async () => {
              setBusy(true);
              setError("");
              try {
                await dispatch({ type: "set_workspace_policy", mode });
              } catch (err) {
                setError(err instanceof Error ? err.message : String(err));
              } finally {
                setBusy(false);
              }
            }}
          >
            {policyLabels[mode]}
          </button>
        ))}
      </div>
      <p className="help-text">
        权限变化会取消当前 Agent 任务并使旧 Agent
        提议失效。恢复允许不会重新执行旧任务。禁止 Agent
        运行仍允许你手动编辑和调整布局。
      </p>
      {snapshot.agent.transport === "stdio" ? (
        <AgentScope snapshot={snapshot} dispatch={dispatch} busy={busy} />
      ) : (
        <p className="help-text">
          当前为 Mock：Host
          控制的文件写入与模块提议遵循此权限；完全访问仍保留路径、文档版本和数据校验。
        </p>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        <button className="button" onClick={onSettings}>
          <Settings2 size={15} />
          前往系统权限设置
        </button>
      </div>
    </Dialog>
  );
}
