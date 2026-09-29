import { useEffect, useState } from "react";
import {
  Activity,
  Check,
  FileText,
  LayoutDashboard,
  MessageSquare,
  CalendarDays,
  ShieldCheck,
  TerminalSquare,
  ArrowRight,
} from "lucide-react";
import type {
  AgentDescriptor,
  AgentProvider,
  ModuleType,
  WorkspaceAction,
  WorkspaceModule,
  WorkspaceSnapshot,
} from "../lib/types";
import { Dialog } from "./Dialog";
import { moveModule } from "../lib/layout";
import { appVersionLabel } from "../lib/app-info";
import type { AppInfoState } from "../lib/app-info";
import {
  agentProvider,
  providerName,
  providerDescriptor,
  providers,
} from "../lib/providers";
import { ProposalDiff, ProposalHistory } from "./ProposalReview";
import { isDesktop } from "../lib/api";
import {
  connectionLabels,
  policyBlockReason,
  policyLabels,
  workspacePolicy,
} from "./PolicyPanel";
import { userFacingError } from "../lib/ui-copy";
function errorMessage(error: unknown) {
  return userFacingError(error);
}
type DisplayConnection = NonNullable<WorkspaceSnapshot["connection"]>;
function connectionHeading(
  name: string,
  connection: DisplayConnection | undefined,
) {
  if (!connection) return "未连接";
  if (connection.status !== "not_installed")
    return connectionLabels[connection.status];
  const probe = connection.probe;
  if (!probe) return `未找到 ${name}`;
  if (!probe.hostAvailable) return `未找到 ${name} 宿主`;
  if (!probe.adapterAvailable) return "未找到 ACP 适配器";
  if (!probe.acpAvailable) return "ACP 检查失败";
  return "连接检查未通过";
}
function connectionFailureHint(
  name: string,
  connection: DisplayConnection | undefined,
) {
  if (!connection) return null;
  if (connection.status === "not_installed") {
    const probe = connection.probe;
    if (probe?.hostAvailable && probe.adapterAvailable && !probe.acpAvailable)
      return "ACP 检查失败 · 查看连接诊断并确认 CLI 或适配器路径。";
    if (probe && !probe.hostAvailable)
      return `未找到 ${name} · 检查 CLI 路径后再探测。`;
    if (probe && !probe.adapterAvailable)
      return "未找到 ACP 适配器 · 检查适配器路径后再探测。";
    return `未找到 ${name} · 查看连接诊断。`;
  }
  if (connection.status === "not_authenticated")
    return `请先在 ${name} 完成登录，再重新连接。`;
  if (connection.status === "error") return "连接失败 · 查看诊断。";
  return null;
}
function diagnosticState(value: string | undefined, kind: "session" | "auth") {
  if (!value || value === "not_checked") return "待检查";
  if (value === "passed") return kind === "session" ? "已连接" : "已认证";
  if (value === "request_succeeded") return "已认证";
  if (value === "auth_required") return "待认证";
  if (value === "failed" || value === "request_failed") return "检查失败";
  return value;
}
export const moduleTypes = [
  {
    type: "conversation" as const,
    title: "对话",
    description: "开始对话",
    icon: MessageSquare,
  },
  {
    type: "planner" as const,
    title: "时间规划",
    description: "规划任务",
    icon: CalendarDays,
  },
  {
    type: "document" as const,
    title: "Markdown 文档",
    description: "编辑内容",
    icon: FileText,
  },
  {
    type: "dashboard" as const,
    title: "数据看板",
    description: "查看数据",
    icon: LayoutDashboard,
  },
];
export function CreatePanel({
  dispatch,
  onClose,
}: {
  dispatch: (a: WorkspaceAction) => Promise<void>;
  onClose: () => void;
}) {
  const [type, setType] = useState<ModuleType>("planner"),
    [title, setTitle] = useState(""),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  return (
    <Dialog title="添加一个模块" onClose={onClose}>
      <div className="type-grid">
        {moduleTypes.map((t) => (
          <button
            key={t.type}
            className={type === t.type ? "chosen" : ""}
            disabled={busy}
            onClick={() => setType(t.type)}
          >
            <t.icon size={22} />
            <strong>{t.title}</strong>
            <small>{t.description}</small>
            {type === t.type && <Check className="type-check" size={15} />}
          </button>
        ))}
      </div>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (busy) return;
          setBusy(true);
          setError("");
          try {
            await dispatch({
              type: "create_module",
              moduleType: type,
              title:
                title.trim() || moduleTypes.find((t) => t.type === type)!.title,
            });
            onClose();
          } catch (err) {
            setError(errorMessage(err));
          } finally {
            setBusy(false);
          }
        }}
      >
        <label className="field">
          模块名称
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder={moduleTypes.find((t) => t.type === type)!.title}
            maxLength={80}
            disabled={busy}
          />
        </label>
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            取消
          </button>
          <button className="button primary" disabled={busy}>
            创建模块
            <ArrowRight size={15} />
          </button>
        </div>
      </form>
    </Dialog>
  );
}
export function ReviewPanel({
  snapshot,
  dispatch,
  onClose,
}: {
  snapshot: WorkspaceSnapshot;
  dispatch: (a: WorkspaceAction) => Promise<void>;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const p = snapshot.approvals[0];
  useEffect(() => setError(""), [p?.id]);
  if (!p)
    return (
      <Dialog title="审批中心" onClose={onClose}>
        <div className="empty-state">
          <ShieldCheck size={32} />
          <h3>暂无待审批</h3>
          {(snapshot.moduleProposals ?? []).length > 0 && <p>历史记录在下方</p>}
        </div>
        <ProposalHistory proposals={snapshot.moduleProposals ?? []} />
      </Dialog>
    );
  const policy = workspacePolicy(snapshot);
  const stale =
    p.origin !== "user" && p.epoch !== undefined && p.epoch !== policy.epoch;
  const blocked =
    p.origin !== "user" &&
    (stale ||
      policy.effective === "restricted" ||
      policy.effective === "disabled");
  const decide = async (allow: boolean) => {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await dispatch({ type: "decide_approval", approvalId: p.id, allow });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const permissionReply = async (optionId: string | null) => {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await dispatch({ type: "permission_reply", approvalId: p.id, optionId });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      title="检查 Agent 提议"
      onClose={onClose}
      wide={p.kind === "write_file" || p.kind === "module_changes"}
    >
      <div className="review-heading">
        <span className="review-icon">
          <ShieldCheck size={24} />
        </span>
        <div>
          <span className="eyebrow">
            需要你的批准 · {snapshot.approvals.length} 项待处理
          </span>
          <h3>{p.title}</h3>
        </div>
      </div>
      <p className="dialog-intro">
        {p.kind === "agent_permission"
          ? `${providerName(snapshot.agent)} 请求一次本机权限。`
          : p.kind === "create_module"
            ? "将在当前工作区创建模块。"
            : p.kind === "write_file"
              ? "检查文件差异后再决定是否写入。"
              : "检查模块差异后再决定是否应用。"}
      </p>
      {p.kind === "module_changes" ? (
        <ProposalDiff approval={p} />
      ) : p.kind === "write_file" ? (
        <>
          <div className="file-label">
            <FileText size={14} />
            {p.filePath}
          </div>
          <dl className="metadata">
            <dt>基于版本</dt>
            <dd>{p.revision || "未提供"}</dd>
          </dl>
          <div className="diff">
            <section>
              <h4>
                <span className="diff-minus">−</span>原文
              </h4>
              <pre>{p.before}</pre>
            </section>
            <section>
              <h4>
                <span className="diff-plus">+</span>修改后
              </h4>
              <pre>{p.after}</pre>
            </section>
          </div>
        </>
      ) : p.kind === "create_module" ? (
        <div className="proposal-card">
          <CalendarDays size={30} />
          <div>
            <strong>{p.title}</strong>
            <p>
              内置{moduleTypes.find((t) => t.type === p.moduleType)?.title}模块
              · 可拖动、编辑与聚焦
            </p>
            <small>将在画布空闲位置创建，并保存模块状态。</small>
          </div>
        </div>
      ) : (
        <div className="permission-scope">
          <strong>{providerName(snapshot.agent)} 请求的权限</strong>
          <p>
            {p.scope ||
              `请检查上方请求说明，再选择 ${providerName(snapshot.agent)} 提供的选项。`}
          </p>
        </div>
      )}
      <details className="proposal-validation-details">
        <summary>查看校验规则</summary>
        <p className="help-text">
          批准前会再次核对当前权限、目标版本、路径和变更范围；模块布局还会检查尺寸与碰撞。
        </p>
        <p className="help-text">原始请求说明：{p.description}</p>
      </details>
      <div className="permission-note">
        <ShieldCheck size={15} />
        {blocked
          ? stale
            ? "权限已变化 · 请重新发起任务"
            : "当前权限不允许执行此 Agent 提议；你仍可拒绝它或手动编辑工作区。"
          : p.kind === "agent_permission"
            ? "仅本次请求；Agent 自有工具仍由其自身控制"
            : "仅本次操作 · 应用前复核版本"}
      </div>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      <ProposalHistory proposals={snapshot.moduleProposals ?? []} />
      <div className="dialog-actions">
        {p.kind === "agent_permission" ? (
          <>
            <button
              className="button"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setError("");
                try {
                  await dispatch({ type: "cancel" });
                } catch (err) {
                  setError(errorMessage(err));
                } finally {
                  setBusy(false);
                }
              }}
            >
              取消任务
            </button>
            <button
              className="button"
              disabled={busy}
              onClick={() => void permissionReply(null)}
            >
              拒绝请求
            </button>
            {(p.options ?? []).map((option) => (
              <button
                key={option.id}
                className={
                  option.kind.startsWith("allow") ? "button primary" : "button"
                }
                disabled={
                  busy || (blocked && !option.kind.startsWith("reject"))
                }
                onClick={() => void permissionReply(option.id)}
              >
                {option.name}
              </button>
            ))}
          </>
        ) : (
          <>
            {p.origin !== "user" && (
              <button
                className="button"
                disabled={busy}
                onClick={async () => {
                  setBusy(true);
                  setError("");
                  try {
                    await dispatch({ type: "cancel" });
                  } catch (err) {
                    setError(errorMessage(err));
                  } finally {
                    setBusy(false);
                  }
                }}
              >
                取消任务
              </button>
            )}
            <button
              className="button"
              disabled={busy}
              onClick={() => void decide(false)}
            >
              拒绝
            </button>
            <button
              className="button primary"
              disabled={busy || blocked}
              onClick={() => void decide(true)}
            >
              <Check size={16} />
              批准并{p.kind === "create_module" ? "创建" : "应用"}
            </button>
          </>
        )}
      </div>
    </Dialog>
  );
}
export function InspectPanel({
  module,
  snapshot,
  dispatch,
  onClose,
}: {
  module: WorkspaceModule;
  snapshot: WorkspaceSnapshot;
  dispatch: (a: WorkspaceAction) => Promise<void>;
  onClose: () => void;
}) {
  const [title, setTitle] = useState(module.title),
    [layout, setLayout] = useState(module.layout),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  return (
    <Dialog title="模块设置" onClose={onClose}>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (busy) return;
          setBusy(true);
          setError("");
          try {
            await dispatch({
              type: "rename_module",
              moduleId: module.id,
              title: title.trim() || module.title,
            });
            await dispatch({
              type: "set_layouts",
              layouts: moveModule(
                snapshot.modules,
                module.id,
                layout,
                snapshot.allowOverlap,
              ),
            });
            onClose();
          } catch (err) {
            setError("设置未全部保存：" + errorMessage(err));
          } finally {
            setBusy(false);
          }
        }}
      >
        <label className="field">
          名称
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            maxLength={80}
            disabled={busy}
          />
        </label>
        <span className="field-label">画布位置与尺寸</span>
        <div className="layout-fields">
          {(["x", "y", "w", "h"] as const).map((k) => (
            <label key={k}>
              {k.toUpperCase()}
              <input
                type="number"
                aria-label={"布局 " + k}
                value={layout[k]}
                disabled={busy}
                min={k === "w" ? 6 : k === "h" ? 24 : 0}
                max={k === "w" || k === "x" ? 24 : k === "h" ? 120 : 10000}
                onChange={(e) =>
                  setLayout({ ...layout, [k]: Number(e.target.value) })
                }
              />
            </label>
          ))}
        </div>
        <p className="help-text">
          24 列网格 · 8 px 行高。也可以选中模块，用方向键移动、Shift +
          方向键缩放。
        </p>
        <dl className="metadata">
          <dt>类型</dt>
          <dd>{module.type}</dd>
          <dt>模块 ID</dt>
          <dd>{module.id}</dd>
          {module.filePath && (
            <>
              <dt>文件</dt>
              <dd>{module.filePath}</dd>
            </>
          )}
        </dl>
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <button className="button primary" disabled={busy}>
            {busy ? "保存中…" : "保存设置"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
export function SettingsPanel({
  appInfo,
  snapshot,
  dispatch,
  onClose,
  onChoose,
  onPolicy,
}: {
  appInfo: AppInfoState;
  snapshot: WorkspaceSnapshot;
  dispatch: (a: WorkspaceAction) => Promise<void>;
  onClose: () => void;
  onChoose: () => void;
  onPolicy?: () => void;
}) {
  const [agent, setAgent] = useState<AgentDescriptor>(snapshot.agent),
    [env, setEnv] = useState(JSON.stringify(snapshot.agent.env)),
    [error, setError] = useState(""),
    [saved, setSaved] = useState(false),
    [busy, setBusy] = useState(false),
    [settingsBusy, setSettingsBusy] = useState(false),
    [settingsError, setSettingsError] = useState("");
  const changeSetting = async (action: WorkspaceAction) => {
    if (settingsBusy) return;
    setSettingsBusy(true);
    setSettingsError("");
    try {
      await dispatch(action);
    } catch (err) {
      setSettingsError(errorMessage(err));
    } finally {
      setSettingsBusy(false);
    }
  };
  const policy = workspacePolicy(snapshot);
  const descriptor = () => {
    const environment: unknown = JSON.parse(env);
    if (
      !environment ||
      typeof environment !== "object" ||
      Array.isArray(environment) ||
      !Object.values(environment).every((value) => typeof value === "string")
    )
      throw new Error("环境变量必须是字符串对象");
    return {
      ...agent,
      args: [...providers[agentProvider(agent)].args],
      env: environment as Record<string, string>,
    };
  };
  let agentDirty = true;
  try {
    const next = descriptor();
    agentDirty = [
      "id",
      "name",
      "transport",
      "command",
      "args",
      "env",
      "cwd",
    ].some(
      (key) =>
        JSON.stringify(next[key as keyof AgentDescriptor]) !==
        JSON.stringify(snapshot.agent[key as keyof AgentDescriptor]),
    );
  } catch {
    /* Keep actions disabled until invalid settings are saved correctly. */
  }
  const displayConnection = agentDirty ? undefined : snapshot.connection;
  const provider = agentProvider(agent);
  const name = providers[provider].name;
  const displayConnectionHeading = connectionHeading(name, displayConnection);
  const displayFailureHint = connectionFailureHint(name, displayConnection);
  const connectionBusy =
    snapshot.connection?.status === "connecting" ||
    snapshot.connection?.status === "stopping";
  const nativeBlocked = policyBlockReason({ ...snapshot, agent });
  const connectionDiagnosticsOpen = [
    "not_installed",
    "not_authenticated",
    "error",
  ].includes(displayConnection?.status ?? "");
  const operateAgent = async (
    type: "probe_agent" | "connect_agent" | "disconnect_agent",
  ) => {
    if (busy) return;
    setBusy(true);
    setError("");
    setSaved(false);
    try {
      if (type === "probe_agent" && agentDirty)
        await dispatch({ type: "save_agent", agent: descriptor() });
      await dispatch({ type });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const agentConfigurationFields = (
    <>
      <label className="field">
        命令路径
        <input
          disabled={busy}
          value={agent.command}
          onChange={(e) => setAgent({ ...agent, command: e.target.value })}
          placeholder={`${providers[provider].command} 或其绝对路径`}
        />
      </label>
      <label className="field">
        启动参数
        <input readOnly value={JSON.stringify(providers[provider].args)} />
      </label>
      <p className="help-text">
        宿主：{providers[provider].host} · ACP 入口：
        {providers[provider].command}
        。已找到命令；连接后显示已连接。
      </p>
      <label className="field">
        环境变量 · JSON
        <input
          disabled={busy}
          value={env}
          onChange={(e) => setEnv(e.target.value)}
        />
        <small className="help-text">
          仅支持 LANG、LC_ALL、TZ、NO_COLOR 等安全字段。请勿输入 API key。
        </small>
      </label>
      <label className="field">
        工作目录
        <input
          disabled={busy}
          value={agent.cwd}
          placeholder="留空使用工作区根目录"
          onChange={(e) => setAgent({ ...agent, cwd: e.target.value })}
        />
      </label>
    </>
  );
  return (
    <Dialog title="工作区设置" onClose={onClose}>
      <details className="settings-section settings-details app-identity">
        <summary>版本与诊断信息 · {appVersionLabel(appInfo)}</summary>
        {appInfo.status === "ready" ? (
          <dl className="metadata">
            <dt>应用名称</dt>
            <dd>{appInfo.info.appName}</dd>
            <dt>体验渠道</dt>
            <dd>{appInfo.info.channel}</dd>
            <dt>基础版本</dt>
            <dd>{appInfo.info.version}</dd>
            <dt>候选编号</dt>
            <dd>{appInfo.info.candidateId ?? "未冻结 · 日常开发"}</dd>
            <dt>构建编号</dt>
            <dd>{appInfo.info.buildId}</dd>
            <dt>源码指纹</dt>
            <dd>{appInfo.info.sourceFingerprint ?? "未冻结"}</dd>
            <dt>版本来源</dt>
            <dd>
              {appInfo.info.source === "native"
                ? "正在运行的本地应用"
                : "当前 Web 构建"}
            </dd>
            <dt>
              {appInfo.info.source === "native" ? "应用标识" : "页面来源"}
            </dt>
            <dd>{appInfo.info.identifier}</dd>
            <dt>
              {appInfo.info.source === "native" ? "运行路径" : "页面地址"}
            </dt>
            <dd>{appInfo.info.executablePath}</dd>
            <dt>
              {appInfo.info.source === "native" ? "应用数据" : "预览数据"}
            </dt>
            <dd>{appInfo.info.dataDir}</dd>
          </dl>
        ) : appInfo.status === "error" ? (
          <p className="inline-error" role="alert">
            版本未确认：{appInfo.message} 请重新打开应用后重试。
          </p>
        ) : (
          <p className="help-text" role="status">
            正在核实运行应用的版本与来源…
          </p>
        )}
      </details>
      <div className="settings-section">
        <h3>工作区</h3>
        <div className="workspace-summary">
          <strong>{snapshot.name}</strong>
          <span>{isDesktop ? "本地" : "预览"}</span>
        </div>
        <p className="path-text">{snapshot.rootPath}</p>
        <button className="button" onClick={onChoose}>
          选择工作区目录
        </button>
      </div>
      <div className="settings-section">
        <h3>
          <TerminalSquare size={17} />
          Agent 连接
        </h3>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            if (busy) return;
            setBusy(true);
            setError("");
            setSaved(false);
            try {
              if (agentDirty)
                await dispatch({ type: "save_agent", agent: descriptor() });
              setSaved(true);
            } catch (err) {
              setError(errorMessage(err));
            } finally {
              setBusy(false);
            }
          }}
        >
          <label className="field">
            连接方式
            <select
              disabled={busy}
              value={provider}
              onChange={(e) => {
                const next = providerDescriptor(
                  e.target.value as AgentProvider,
                  snapshot.agent,
                );
                setAgent(next);
                setEnv(JSON.stringify(next.env));
                setSaved(false);
              }}
            >
              <option value="mock">Mock Agent · 内置演示</option>
              <option value="hermes">Hermes · 本机 ACP</option>
              <option value="claude_code">Claude Code · 本机 ACP</option>
              <option value="codex">Codex · 本机 ACP</option>
            </select>
          </label>
          {agent.transport === "stdio" && !isDesktop && (
            <p className="inline-warning">
              Web 预览仅模拟权限和 Mock。真实 {name} 需要在 Dev 或 Beta
              原生应用中连接。
            </p>
          )}
          <label className="field">
            名称
            <input
              disabled={busy}
              value={agent.name}
              onChange={(e) => setAgent({ ...agent, name: e.target.value })}
            />
          </label>
          {agent.transport === "stdio" &&
            (provider === "codex" ? (
              <>
                <p className="help-text" data-testid="codex-runtime-help">
                  Codex 连接时自动检查；Atrio 不安装 CLI，默认配置即可。
                </p>
                <p className="help-text" data-testid="codex-auth-help">
                  复用本机 Codex 登录；Atrio 不保存密钥。待登录时先在 Codex
                  完成登录。
                </p>
                <details data-testid="codex-advanced">
                  <summary>高级连接设置</summary>
                  <p className="help-text">
                    外部适配器、命令参数、环境变量和工作目录。
                  </p>
                  {agentConfigurationFields}
                </details>
              </>
            ) : (
              <details data-testid="advanced-connection">
                <summary>高级连接设置</summary>
                {agentConfigurationFields}
              </details>
            ))}
          {agent.transport === "stdio" ? (
            <>
              {agentDirty ? (
                <p className="inline-warning">
                  先保存 {name}{" "}
                  配置，再确认本工作区的运行范围并连接。修改命令或配置会撤销之前的运行确认。
                </p>
              ) : (
                <div className="agent-scope-link">
                  <span>
                    {nativeBlocked ||
                      `当前权限：${policyLabels[policy.effective]}。`}
                  </span>
                  <button
                    type="button"
                    className="button"
                    onClick={onPolicy}
                    disabled={!onPolicy}
                  >
                    打开权限与安全
                  </button>
                </div>
              )}
              <div className="connection-state" role="status">
                <strong>{displayConnectionHeading}</strong>
                <p className="connection-summary">
                  宿主：
                  {displayConnection?.probe
                    ? displayConnection.probe.hostAvailable
                      ? "已找到"
                      : "未找到"
                    : "待检查"}
                  {" · ACP："}
                  {displayConnection?.probe
                    ? displayConnection.probe.adapterAvailable
                      ? "已找到"
                      : "未找到"
                    : "待检查"}
                  {" · 会话："}
                  {displayConnection?.status === "connected"
                    ? "已连接"
                    : diagnosticState(
                        displayConnection?.probe?.sessionStatus,
                        "session",
                      )}
                  {" · 认证："}
                  {diagnosticState(
                    displayConnection?.probe?.authenticationStatus,
                    "auth",
                  )}
                </p>
                <p>
                  {displayFailureHint ||
                    (displayConnection?.status === "connected"
                      ? "已连接"
                      : provider === "codex"
                        ? "确认运行范围后连接；连接时自动检查。"
                        : `检查本机 ${name} 后连接。`)}
                </p>
                {(displayConnection || error) && (
                  <details
                    className="connection-diagnostics"
                    data-testid="connection-diagnostics"
                    open={connectionDiagnosticsOpen || !!error}
                  >
                    <summary>连接诊断</summary>
                    {displayConnection?.message && (
                      <p className="path-text">{displayConnection.message}</p>
                    )}
                    {error && <p className="inline-error">{error}</p>}
                    {(displayConnection?.providerVersion ||
                      (provider === "hermes" &&
                        displayConnection?.hermesVersion)) && (
                      <p>
                        {name} 入口版本：
                        {displayConnection?.providerVersion ||
                          displayConnection?.hermesVersion}
                      </p>
                    )}
                    {displayConnection?.providerSessionId && (
                      <p className="path-text">
                        会话：{displayConnection.providerSessionId}
                      </p>
                    )}
                    {displayConnection?.probe && (
                      <>
                        <p>
                          宿主：
                          {displayConnection.probe.hostAvailable
                            ? "已找到"
                            : "未找到"}{" "}
                          ·{" "}
                          {displayConnection.probe.hostVersion || "版本未确认"}
                        </p>
                        <p className="path-text">
                          {displayConnection.probe.hostExecutable}
                        </p>
                        <p>
                          ACP 入口：
                          {displayConnection.probe.adapterAvailable
                            ? "已找到"
                            : "未找到"}{" "}
                          · {displayConnection.probe.version || "版本未确认"}
                        </p>
                        <p className="path-text">
                          {displayConnection.probe.executable}
                        </p>
                        <p>{displayConnection.probe.detail}</p>
                        <p>
                          ACP 检查：
                          {displayConnection.probe.acpAvailable
                            ? "通过"
                            : "未通过"}
                        </p>
                        <p>
                          握手：{displayConnection.probe.handshakeStatus}
                          ；会话：
                          {displayConnection.probe.sessionStatus}
                          ；认证/服务调用：
                          {displayConnection.probe.authenticationStatus}
                        </p>
                      </>
                    )}
                  </details>
                )}
              </div>
              <details className="connection-notes">
                <summary>连接与安全说明</summary>
                <p className="help-text">
                  重连后建立新会话；历史记录保存在本地，请重新提供上下文。
                </p>
              </details>
            </>
          ) : (
            <div className="agent-capabilities">
              <span>✓ Mock 流式演示</span>
              <span>✓ Host 权限审批</span>
              <span>— 不启动外部 Agent</span>
            </div>
          )}
          {error && (
            <p role="alert" className="inline-error">
              {error}
            </p>
          )}
          {saved && <p className="success-text">Agent 配置已保存。</p>}
          <div className="settings-buttons">
            <button
              type="button"
              className="button"
              disabled={
                busy ||
                connectionBusy ||
                (agent.transport === "stdio" && !isDesktop)
              }
              onClick={() => void operateAgent("probe_agent")}
            >
              探测连接
            </button>
            <button
              className="button primary"
              disabled={busy || connectionBusy}
            >
              {busy ? "处理中…" : "保存 Agent"}
            </button>
            {agent.transport === "stdio" && (
              <>
                <button
                  type="button"
                  className="button primary"
                  disabled={
                    busy ||
                    agentDirty ||
                    connectionBusy ||
                    !isDesktop ||
                    !!nativeBlocked ||
                    displayConnection?.status === "connected"
                  }
                  onClick={() => void operateAgent("connect_agent")}
                >
                  连接 {name}
                </button>
                <button
                  type="button"
                  className="button"
                  disabled={
                    busy ||
                    agentDirty ||
                    !isDesktop ||
                    !["connected", "connecting", "error"].includes(
                      displayConnection?.status ?? "disconnected",
                    )
                  }
                  onClick={() => void operateAgent("disconnect_agent")}
                >
                  断开 {name}
                </button>
              </>
            )}
          </div>
          <p className="help-text">探测状态：{snapshot.agent.probeStatus}</p>
        </form>
      </div>
      <div className="settings-section">
        <h3>
          <ShieldCheck size={17} />
          系统权限策略
        </h3>
        <div
          className="policy-options system-policy"
          role="group"
          aria-label="系统权限策略"
        >
          {(
            [
              ["workspace", "按工作区设置"],
              ["allow_all", "全部允许"],
              ["deny_all", "全部不允许"],
            ] as const
          ).map(([mode, label]) => (
            <button
              key={mode}
              className={policy.system === mode ? "chosen" : ""}
              aria-pressed={policy.system === mode}
              disabled={settingsBusy}
              onClick={() =>
                void changeSetting({ type: "set_system_policy", mode })
              }
            >
              {label}
            </button>
          ))}
        </div>
        <p className="help-text">
          当前生效：{policyLabels[policy.effective]} ·{" "}
          {policy.source === "system" ? "系统" : "工作区"}
          。系统策略作用于当前应用渠道的所有工作区；全部不允许仍可手动编辑。
        </p>
        <label className="toggle-row">
          <span>允许模块重叠</span>
          <input
            type="checkbox"
            checked={snapshot.allowOverlap}
            disabled={settingsBusy}
            onChange={(e) =>
              void changeSetting({
                type: "set_overlap",
                allow: e.target.checked,
              })
            }
          />
        </label>
        {settingsError && (
          <p className="inline-error" role="alert">
            {settingsError}
          </p>
        )}
      </div>
    </Dialog>
  );
}
function eventSummary(kind: string) {
  if (/fail|error/.test(kind)) return "操作失败";
  if (/permission|approval/.test(kind)) return "权限与审批";
  if (/proposal|module/.test(kind)) return "模块变更";
  if (/connect|session/.test(kind)) return "连接状态";
  if (/prompt|message|task/.test(kind)) return "任务进展";
  return "工作区变更";
}
export function EventList({ snapshot }: { snapshot: WorkspaceSnapshot }) {
  const events = [...snapshot.events].reverse();
  return (
    <div className="event-view">
      <div className="page-heading">
        <div>
          <span className="eyebrow">ACTIVITY LOG</span>
          <h1>运行记录</h1>
        </div>
        <span className="outlined-badge">
          <Activity size={14} />
          {events.length} 条近期事件
        </span>
      </div>
      <div className="event-table">
        <div className="event-table-head">
          <span>序号 / 时间</span>
          <span>事件</span>
          <span>详情</span>
        </div>
        {events.map((e) => (
          <div className="event-row" key={e.seq}>
            <span className="event-time">
              #{String(e.seq).padStart(3, "0")}
              <small>
                {new Date(e.timestamp).toLocaleTimeString("zh-CN", {
                  hour12: false,
                })}
              </small>
            </span>
            <strong
              className={
                /fail|error/.test(e.kind)
                  ? "danger-text"
                  : /permission|approval/.test(e.kind)
                    ? "amber-text"
                    : ""
              }
            >
              {eventSummary(e.kind)}
            </strong>
            <details className="event-details">
              <summary>查看详情</summary>
              <code>{e.kind}</code>
              <p>{e.message}</p>
              {(e.actor ||
                e.sessionId ||
                e.connectorSeq != null ||
                e.context) && (
                <dl className="event-raw" aria-label="原始事件字段">
                  {e.actor && (
                    <>
                      <dt>来源</dt>
                      <dd>{e.actor}</dd>
                    </>
                  )}
                  {e.sessionId && (
                    <>
                      <dt>会话</dt>
                      <dd>{e.sessionId}</dd>
                    </>
                  )}
                  {e.connectorSeq != null && (
                    <>
                      <dt>连接序号</dt>
                      <dd>{e.connectorSeq}</dd>
                    </>
                  )}
                  {e.context && (
                    <>
                      <dt>运行上下文</dt>
                      <dd>
                        <pre>{JSON.stringify(e.context, null, 2)}</pre>
                      </dd>
                    </>
                  )}
                </dl>
              )}
            </details>
          </div>
        ))}
      </div>
    </div>
  );
}
