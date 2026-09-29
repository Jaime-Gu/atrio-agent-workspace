import { useState } from "react";
import { Settings2, ShieldCheck } from "lucide-react";
import type {
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
import { connectionStatusLabels, policyModeLabels } from "../lib/ui-copy";

export const policyLabels = policyModeLabels;
export const connectionLabels = connectionStatusLabels;
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
    return "Agent 已停用 · 仍可手动编辑和调整布局";
  if (snapshot.agent.transport === "stdio") {
    if (policy.effective === "restricted")
      return "只读模式暂不可连接 · 查看权限";
    if (policy.effective === "ask" && !scopeAccepted(policy, snapshot.agent))
      return "先确认 Agent 运行范围 · 打开权限与安全";
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
    <div className="agent-scope hermes-scope">
      <p className="agent-scope-summary">
        {name} 在本机运行；工作区模块修改由 Host 校验并按规则审批。
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
          <span>允许 {name} 在此工作区运行；它的自有工具可能访问本机。</span>
        </label>
      )}
      {policy.effective === "full" && (
        <p className="policy-consequence">
          内容修改可自动应用；布局变更仍需确认。
        </p>
      )}
      {policy.effective === "restricted" && (
        <p className="policy-consequence">
          只读模式暂不可连接；请查看权限与安全。
        </p>
      )}
      <details className="policy-details">
        <summary>运行范围</summary>
        <div>
          <p>
            {name} 从当前工作目录启动；文件、命令和网络能力取决于 Agent
            自身设置。工作区模块写入仍经过 Host 的路径、revision、schema
            和审批校验。
          </p>
          <p>{providers[provider].profile}</p>
          <p>
            {name} 自有工具仅在它实际发来权限请求时可审批；ACP
            权限答复只覆盖当前请求。
          </p>
        </div>
      </details>
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
    <Dialog title="权限与安全" onClose={onClose}>
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
      <p className="path-text">工作区：{snapshot.rootPath}</p>
      <p className="policy-intro">
        {snapshot.agent.transport === "stdio"
          ? `${providerName(snapshot.agent)} 在本机运行；工作区模块修改由 Host 校验并按规则审批。`
          : "当前 Agent 使用本地演示；工作区模块修改仍由 Host 校验并按规则审批。"}
      </p>
      {policy.source === "system" && (
        <p className="inline-warning">
          系统策略覆盖当前渠道的所有工作区。原工作区选择“
          {policyLabels[policy.local]}”已保留，恢复“按工作区设置”后生效。
        </p>
      )}
      {!policy.workspaceTrusted && (
        <p className="inline-warning">
          此工作区尚未确认局部权限；复制或迁移后需重新确认。
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
      <p className="policy-consequence">
        权限变更会停止当前 Agent 任务并使旧提案失效；手动编辑仍可用。
      </p>
      {snapshot.agent.transport === "stdio" ? (
        <AgentScope snapshot={snapshot} dispatch={dispatch} busy={busy} />
      ) : (
        <p className="agent-scope-summary">
          当前为本地演示；Host 控制的文件写入与模块提议遵循此权限。
        </p>
      )}
      <div className="policy-details-list">
        <details className="policy-details">
          <summary>权限与撤销</summary>
          <p>
            系统策略覆盖当前渠道的所有工作区；恢复“按工作区设置”后各工作区的原选择重新生效。撤权不会重放旧任务。
          </p>
        </details>
        <details className="policy-details">
          <summary>数据与上下文</summary>
          <p>
            本轮默认引用已保存模块版本；未保存草稿不进入本轮。历史保存在本地，重连后建立新会话，请重新提供上下文。
          </p>
        </details>
        <details className="policy-details">
          <summary>审批与记录</summary>
          <p>提案批准后才写入；事件和审批记录可在运行记录中查看。</p>
        </details>
        <details className="policy-details">
          <summary>Provider 工具</summary>
          <p>
            ACP 权限答复只覆盖当前请求；Agent 自有工具的能力由 Provider 控制。
          </p>
        </details>
      </div>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        <button className="button" onClick={onSettings}>
          <Settings2 size={15} />
          设置与连接
        </button>
      </div>
    </Dialog>
  );
}
