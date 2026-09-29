import { useCallback, useEffect, useRef, useState } from "react";
import {
  Activity,
  ArrowDown,
  ArrowRight,
  Check,
  ChevronDown,
  Command,
  FileText,
  Folder,
  Grid2X2,
  LayoutGrid,
  LoaderCircle,
  Plus,
  Search,
  Settings2,
  ShieldCheck,
  Square,
  X,
  Zap,
} from "lucide-react";
import type { WorkspaceAction, WorkspaceSnapshot } from "./lib/types";
import {
  dispatchAction,
  getWorkspace,
  isDesktop,
  openWorkspace,
  subscribe,
} from "./lib/api";
import { arrangeModules } from "./lib/layout";
import {
  PRODUCT_NAME,
  appInfoError,
  appVersionLabel,
  initialAppInfo,
  loadAppInfo,
} from "./lib/app-info";
import { providerName } from "./lib/providers";
import type { AppInfoState } from "./lib/app-info";
import { Canvas } from "./components/Canvas";
import { Dialog } from "./components/Dialog";
import {
  connectionLabels,
  PolicyPanel,
  policyBlockReason,
  policyLabels,
  workspacePolicy,
} from "./components/PolicyPanel";
import {
  CreatePanel,
  EventList,
  InspectPanel,
  ReviewPanel,
  SettingsPanel,
  moduleTypes,
} from "./components/Panels";
const runLabels = {
  idle: "等待新想法",
  running: "Agent 正在工作",
  waiting_approval: "等待你的审批",
  completed: "任务已完成",
  failed: "任务未完成",
  cancelled: "任务已取消",
};
export default function App() {
  const [appInfo, setAppInfo] = useState<AppInfoState>(initialAppInfo);
  const versionLabel = appVersionLabel(appInfo);
  useEffect(() => {
    if (!isDesktop) return;
    let disposed = false;
    void loadAppInfo().then(
      (info) => {
        if (!disposed) setAppInfo({ status: "ready", info });
      },
      (error: unknown) => {
        if (!disposed) setAppInfo(appInfoError(error));
      },
    );
    return () => {
      disposed = true;
    };
  }, []);
  useEffect(() => {
    document.title = `${appInfo.status === "ready" ? appInfo.info.appName : PRODUCT_NAME} · ${versionLabel}`;
  }, [appInfo, versionLabel]);
  const [snapshot, setSnapshot] = useState<WorkspaceSnapshot | null>(null),
    [error, setError] = useState(""),
    [selected, setSelected] = useState<string | null>(null),
    [focused, setFocused] = useState<string | null>(null),
    [view, setView] = useState<"board" | "events" | "files">("board");
  const [panel, setPanel] = useState<
      "create" | "review" | "settings" | "command" | "policy" | null
    >(null),
    [inspect, setInspect] = useState<string | null>(null),
    [input, setInput] = useState(""),
    [search, setSearch] = useState(""),
    [sending, setSending] = useState(false),
    [saved, setSaved] = useState(false),
    [switching, setSwitching] = useState(false);
  const currentSnapshot = useRef<WorkspaceSnapshot | null>(null),
    switchingWorkspace = useRef(false),
    workspaceGeneration = useRef(0);
  const applySnapshot = useCallback(
    (next: WorkspaceSnapshot, activate = false) => {
      const previous = currentSnapshot.current;
      const sameWorkspace =
        previous &&
        next.rootPath === previous.rootPath &&
        next.workspaceGeneration === previous.workspaceGeneration;
      // A high sequence number cannot authorize an event from an earlier A → B → A visit.
      // Only an explicit open/get response may activate another Host generation.
      if (previous && !sameWorkspace && !activate) return false;
      if (previous && sameWorkspace) {
        const older =
          Date.parse(next.updatedAt) < Date.parse(previous.updatedAt);
        const nextSeq = next.events.at(-1)?.seq || 0;
        const previousSeq = previous.events.at(-1)?.seq || 0;
        if (nextSeq < previousSeq || (nextSeq === previousSeq && older))
          return false;
      }
      currentSnapshot.current = next;
      setSnapshot(next);
      return true;
    },
    [],
  );
  const inputRef = useRef<HTMLTextAreaElement>(null),
    seenApproval = useRef(""),
    alive = useRef(true),
    saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => {
    let disposed = false;
    alive.current = true;
    let unsub: (() => void) | undefined;
    let errorUnsub: (() => void) | undefined;
    void (async () => {
      try {
        unsub = await subscribe((s) => {
          if (!disposed) applySnapshot(s);
        });
        if (disposed) {
          unsub();
          return;
        }
        const generation = workspaceGeneration.current;
        const s = await getWorkspace();
        if (!disposed && generation === workspaceGeneration.current) {
          applySnapshot(s, true);
          setSelected(s.modules[0]?.id || null);
        }
        if (isDesktop) {
          errorUnsub = await (
            await import("@tauri-apps/api/event")
          ).listen<string>("workspace://error", (e) =>
            setError(String(e.payload)),
          );
          if (disposed) errorUnsub();
        }
      } catch (e) {
        setError(String(e));
      }
    })();
    return () => {
      disposed = true;
      alive.current = false;
      unsub?.();
      errorUnsub?.();
      clearTimeout(saveTimer.current);
    };
  }, []);
  useEffect(() => {
    if (
      snapshot?.approvals[0] &&
      snapshot.approvals[0].id !== seenApproval.current
    ) {
      seenApproval.current = snapshot.approvals[0].id;
      setPanel("review");
    }
  }, [snapshot?.approvals]);
  useEffect(() => {
    const listener = (e: KeyboardEvent) => {
      if (switchingWorkspace.current) return;
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPanel((p) => (p === "command" ? null : "command"));
        setSearch("");
      }
      if (e.key === "Escape" && !document.querySelector("dialog[open]"))
        setFocused(null);
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "j") {
        e.preventDefault();
        inputRef.current?.focus();
      }
    };
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);
  useEffect(() => {
    if (selected && !snapshot?.modules.some((m) => m.id === selected))
      setSelected(snapshot?.modules[0]?.id || null);
    if (focused && !snapshot?.modules.some((m) => m.id === focused))
      setFocused(null);
  }, [snapshot?.modules, selected, focused]);
  // Async handlers keep the workspace identity of the render that created them.
  // This also prevents a multi-step form from continuing in a copied workspace.
  const renderGeneration = workspaceGeneration.current;
  const dispatch = useCallback(
    async (action: WorkspaceAction) => {
      const generation = renderGeneration;
      try {
        if (
          generation !== workspaceGeneration.current ||
          snapshot?.rootPath !== currentSnapshot.current?.rootPath ||
          snapshot?.workspaceGeneration !==
            currentSnapshot.current?.workspaceGeneration
        )
          throw new Error("工作区已切换，此操作已取消。");
        if (switchingWorkspace.current)
          throw new Error("正在切换工作区，请稍候再操作。");
        const root = snapshot?.rootPath;
        if (!root) throw new Error("工作区尚未加载。");
        const hostGeneration = snapshot?.workspaceGeneration;
        const s = await dispatchAction(action, root, hostGeneration);
        if (
          generation !== workspaceGeneration.current ||
          s.rootPath !== root ||
          s.workspaceGeneration !== hostGeneration
        )
          throw new Error("工作区已切换，后续操作已取消。");
        applySnapshot(s);
        setSaved(true);
        clearTimeout(saveTimer.current);
        saveTimer.current = setTimeout(() => setSaved(false), 1800);
      } catch (e) {
        if (generation === workspaceGeneration.current) setError(String(e));
        throw e;
      }
    },
    [
      snapshot?.rootPath,
      snapshot?.workspaceGeneration,
      renderGeneration,
      applySnapshot,
    ],
  );
  // UI event handlers report rejected actions through the shared alert, without unhandled rejections.
  const uiDispatch = useCallback(
    async (action: WorkspaceAction) => {
      try {
        await dispatch(action);
      } catch {
        /* shared alert */
      }
    },
    [dispatch],
  );
  const prompt = useCallback((text: string, id?: string) => {
    setInput(text);
    if (id) setSelected(id);
    inputRef.current?.focus();
  }, []);
  const send = async () => {
    if (
      !input.trim() ||
      switchingWorkspace.current ||
      sending ||
      snapshot?.runStatus === "running" ||
      snapshot?.approvals.length
    )
      return;
    if (snapshot && policyBlockReason(snapshot)) {
      setPanel("policy");
      return;
    }
    if (
      snapshot?.agent.transport === "stdio" &&
      (!isDesktop || snapshot.connection?.status !== "connected")
    ) {
      setPanel("settings");
      return;
    }
    setSending(true);
    const generation = workspaceGeneration.current;
    try {
      await dispatch({
        type: "prompt",
        text: input.trim(),
        moduleId: selected,
      });
      if (generation === workspaceGeneration.current) setInput("");
    } catch {
      /* retain prompt on failure */
    } finally {
      if (generation === workspaceGeneration.current) setSending(false);
    }
  };
  const chooseWorkspace = async () => {
    if (switchingWorkspace.current) return;
    switchingWorkspace.current = true;
    setSwitching(true);
    try {
      if (!isDesktop) {
        setError("网页预览使用浏览器存储。请在桌面应用中选择本地工作区。");
        return;
      }
      const path = await (
        await import("@tauri-apps/plugin-dialog")
      ).open({ directory: true, multiple: false, title: "选择个人工作区目录" });
      if (typeof path === "string") {
        workspaceGeneration.current += 1;
        const s = await openWorkspace(path);
        applySnapshot(s, true);
        setSelected(s.modules[0]?.id || null);
        setFocused(null);
        setPanel(null);
        setInspect(null);
        setInput("");
        setError("");
        setSaved(false);
        setSending(false);
        clearTimeout(saveTimer.current);
        setView("board");
      }
    } catch (e) {
      setError(String(e));
      // A failed switch may already have cancelled the previous run.
      // Reconcile with the Host instead of retaining a stale running screen.
      try {
        applySnapshot(await getWorkspace(), true);
        setSending(false);
      } catch {
        /* Keep the original switch error and the last readable workspace. */
      }
    } finally {
      switchingWorkspace.current = false;
      setSwitching(false);
    }
  };
  const focusModule = (id: string | null) => {
    setFocused(id);
    if (id) {
      setSelected(id);
      setView("board");
    }
  };
  if (!snapshot)
    return (
      <main className="loading-screen">
        <div className="brand-glyph">▦</div>
        <h1>{PRODUCT_NAME}</h1>
        <span className="version" data-version-status={appInfo.status}>
          {versionLabel}
        </span>
        <p>{error || "正在打开你的工作区…"}</p>
        {error && (
          <>
            <button className="button" onClick={() => location.reload()}>
              重试
            </button>
            <button
              className="button"
              disabled={switching}
              onClick={() => void chooseWorkspace()}
            >
              选择其他工作区
            </button>
          </>
        )}
      </main>
    );
  const active = snapshot.modules.find((m) => m.id === selected),
    activeRun =
      snapshot.runStatus === "running" ||
      snapshot.approvals.some((approval) => approval.origin !== "user"),
    pending = snapshot.approvals.length;
  const policy = workspacePolicy(snapshot);
  const agentBlock =
    policyBlockReason(snapshot) ||
    (snapshot.agent.transport === "stdio" &&
      (!isDesktop
        ? `真实 ${providerName(snapshot.agent)} 需要在原生应用中连接。`
        : snapshot.connection?.status !== "connected"
          ? snapshot.connection?.message ||
            `请先在设置中连接 ${providerName(snapshot.agent)}。`
          : null));
  const today = new Intl.DateTimeFormat("zh-CN", {
    month: "long",
    day: "numeric",
    weekday: "long",
  }).format(new Date());
  return (
    <>
      <div className="app-shell" inert={switching} aria-busy={switching}>
        <header className="topbar">
          <div className="brand">
            <span className="brand-glyph">
              <Grid2X2 size={25} />
            </span>
            <span>Atrio</span>
            <span
              className="version"
              data-version-status={appInfo.status}
              aria-label={`当前版本：${versionLabel}`}
              title={
                appInfo.status === "error"
                  ? appInfo.message
                  : "在设置中查看当前应用的版本与来源"
              }
            >
              {versionLabel}
            </span>
          </div>
          <div className="topbar-context">
            <span className="breadcrumb">个人空间</span>
            <span className="slash">/</span>
            <strong>{snapshot.name}</strong>
            <span className="local-badge">LOCAL</span>
          </div>
          <button className="agent-status" onClick={() => setPanel("settings")}>
            <span
              className={
                "status-square " +
                (activeRun
                  ? "running"
                  : snapshot.agent.transport === "mock" ||
                      snapshot.connection?.status === "connected"
                    ? "idle"
                    : "offline")
              }
            />
            <span>
              {snapshot.agent.transport === "mock"
                ? "Mock Agent"
                : snapshot.agent.name}{" "}
              <small>
                {snapshot.agent.transport === "mock"
                  ? "本地演示 · 无需 API Key"
                  : connectionLabels[
                      snapshot.connection?.status ?? "disconnected"
                    ]}
              </small>
            </span>
            <ChevronDown size={14} />
          </button>
          <button
            className="icon-button top-settings"
            aria-label="打开设置"
            onClick={() => setPanel("settings")}
          >
            <Settings2 size={19} />
          </button>
        </header>
        <aside className="sidebar">
          <button
            className="workspace-switch"
            onClick={() => void chooseWorkspace()}
          >
            <span className="workspace-avatar">A</span>
            <span>
              <strong>{snapshot.name}</strong>
              <small>个人工作区</small>
            </span>
            <ChevronDown size={14} />
          </button>
          <span className="nav-label">WORKSPACE</span>
          <nav>
            <button
              className={view === "board" ? "active" : ""}
              onClick={() => {
                setView("board");
                setFocused(null);
              }}
            >
              <LayoutGrid size={17} />
              <span>工作台</span>
              <span className="nav-count">{snapshot.modules.length}</span>
            </button>
            <button
              className={view === "files" ? "active" : ""}
              onClick={() => {
                setView("files");
                setFocused(null);
              }}
            >
              <Folder size={17} />
              <span>工作区文件</span>
            </button>
            <button
              className={view === "events" ? "active" : ""}
              onClick={() => {
                setView("events");
                setFocused(null);
              }}
            >
              <Activity size={17} />
              <span>运行记录</span>
              {activeRun && <span className="pulsing-dot" />}
            </button>
          </nav>
          <div className="sidebar-section-heading">
            <span className="nav-label">我的模块</span>
            <button
              className="icon-button"
              aria-label="添加模块"
              onClick={() => setPanel("create")}
            >
              <Plus size={16} />
            </button>
          </div>
          <div className="module-nav">
            {snapshot.modules.map((m) => {
              const Icon = moduleTypes.find((t) => t.type === m.type)!.icon;
              return (
                <button
                  key={m.id}
                  className={selected === m.id ? "selected" : ""}
                  onClick={() => {
                    setSelected(m.id);
                    setView("board");
                    setFocused(null);
                    requestAnimationFrame(() =>
                      document
                        .querySelector(`[data-module-id="${m.id}"]`)
                        ?.scrollIntoView({
                          block: "nearest",
                          behavior: "smooth",
                        }),
                    );
                  }}
                >
                  <Icon size={15} />
                  <span>{m.title}</span>
                  <i className={"tiny-dot " + m.status} />
                </button>
              );
            })}
          </div>
          <button className="new-module" onClick={() => setPanel("create")}>
            <Plus size={15} />
            添加模块
          </button>
          <div className="sidebar-bottom">
            <div className="local-info">
              <span className="status-square idle" />
              <span>
                你的工作，保存在本地
                <small>
                  {isDesktop ? "SQLite · 文件工作区" : "浏览器交互预览"}
                </small>
              </span>
            </div>
            <button
              className="command-shortcut"
              onClick={() => {
                setSearch("");
                setPanel("command");
              }}
            >
              <Search size={14} />
              <span>快速查找</span>
              <kbd>⌘ K</kbd>
            </button>
            <button
              className="settings-link"
              onClick={() => setPanel("settings")}
            >
              <Settings2 size={15} />
              设置与连接<span>{versionLabel}</span>
            </button>
          </div>
        </aside>
        <button
          className="workspace-policy-button"
          aria-label={`工作区权限：${policyLabels[policy.effective]} · ${policy.source === "system" ? "系统" : "工作区"}`}
          onClick={() => setPanel("policy")}
        >
          <ShieldCheck size={18} />
          <span>
            {policyLabels[policy.effective]}
            <small>
              来源：{policy.source === "system" ? "系统" : "工作区"}
            </small>
          </span>
          <ChevronDown size={13} />
        </button>
        <main className="main-area">
          {view === "board" && (
            <>
              <div className="board-heading">
                <div>
                  <span className="eyebrow">
                    {focused ? "FOCUS MODE" : "YOUR PERSONAL CANVAS"}
                  </span>
                  <h1>{focused ? active?.title : "让想法，各就其位。"}</h1>
                  <p>
                    {focused
                      ? "专注于当前模块，按 Esc 返回原来的工作台。"
                      : `${today} · 从一个目标开始，让工作在这里展开。`}
                  </p>
                </div>
                <div className="board-actions">
                  {(pending > 0 || !!snapshot.moduleProposals?.length) && (
                    <button
                      className="button approval-button"
                      onClick={() => setPanel("review")}
                    >
                      <ShieldCheck size={15} />
                      {pending ? `${pending} 项待审批` : "提案记录"}
                    </button>
                  )}
                  {focused ? (
                    <button className="button" onClick={() => setFocused(null)}>
                      返回画布 <kbd>ESC</kbd>
                    </button>
                  ) : (
                    <>
                      <button
                        className="icon-button arrange-button"
                        title="整理为两列布局"
                        aria-label="整理布局"
                        onClick={() =>
                          void uiDispatch({
                            type: "set_layouts",
                            layouts: arrangeModules(snapshot.modules),
                          })
                        }
                      >
                        <LayoutGrid size={18} />
                      </button>
                      <button
                        className="button primary"
                        onClick={() => setPanel("create")}
                      >
                        <Plus size={17} />
                        添加模块
                      </button>
                    </>
                  )}
                </div>
              </div>
              <div className="board-meta">
                <span>
                  <span className="violet-square" />
                  {focused ? "专注视图" : "默认画布"}
                  <i /> {snapshot.modules.length} 个模块
                </span>
                <span className="board-hint">
                  {saved ? (
                    <>
                      <Check size={12} /> 已保存
                    </>
                  ) : (
                    <>拖动标题移动 · 右下角缩放 · 双击聚焦</>
                  )}
                </span>
                <span className="grid-label">
                  24 COL <Grid2X2 size={12} />
                </span>
              </div>
              <Canvas
                key={snapshot.rootPath}
                snapshot={snapshot}
                selected={selected}
                onSelect={setSelected}
                focused={focused}
                onFocus={focusModule}
                dispatch={uiDispatch}
                contentDispatch={dispatch}
                onPrompt={prompt}
                onInspect={setInspect}
              />
            </>
          )}
          {view === "events" && <EventList snapshot={snapshot} />}
          {view === "files" && (
            <div className="files-view">
              <div className="page-heading">
                <div>
                  <span className="eyebrow">WORKSPACE FILES</span>
                  <h1>成果，留在你的手里。</h1>
                  <p>文档以 Markdown 文件保存在工作区，可直接打开和备份。</p>
                </div>
                <button
                  className="button"
                  onClick={() => void chooseWorkspace()}
                >
                  <Folder size={15} />
                  选择目录
                </button>
              </div>
              <div className="root-path">
                <Folder size={16} />
                {snapshot.rootPath}
              </div>
              <div className="file-list">
                {snapshot.modules
                  .filter((m) => m.filePath)
                  .map((m) => (
                    <button key={m.id} onClick={() => focusModule(m.id)}>
                      <span className="file-icon">
                        <FileText size={23} />
                      </span>
                      <span>
                        <strong>{m.title}</strong>
                        <small>{m.filePath}</small>
                      </span>
                      <span className="file-type">MARKDOWN</span>
                      <ArrowRight size={17} />
                    </button>
                  ))}
                {!snapshot.modules.some((m) => m.filePath) && (
                  <p className="empty-state">
                    还没有文档。添加一个文档模块即可开始。
                  </p>
                )}
              </div>
              <p className="help-text">
                当前显示已关联模块的文件。完整文件索引与搜索将在后续版本加入。
              </p>
            </div>
          )}
        </main>
        <footer className="composer-bar">
          <div className="composer-context">
            <span className="composer-logo">
              <Command size={20} />
            </span>
            <div>
              <span className="eyebrow">与 AGENT 协作</span>
              <button
                title="本轮引用所选模块的已保存版本。Agent 可按权限调用工具读取当前工作区模块；未保存草稿不会发送。"
                onClick={() => {
                  if (active) focusModule(active.id);
                }}
              >
                {active?.title || "工作区"}
                <ChevronDown size={12} />
              </button>
            </div>
          </div>
          <div className={"composer " + (activeRun ? "is-running" : "")}>
            {agentBlock && (
              <button
                className="composer-policy-note"
                onClick={() =>
                  setPanel(policyBlockReason(snapshot) ? "policy" : "settings")
                }
              >
                {agentBlock}
              </button>
            )}
            <p
              className="composer-context-note"
              data-testid="context-scope"
              title={`本轮上下文：${active ? `「${active.title}」已保存版本` : "未选择模块"}；Agent 可以按权限使用当前工作区模块工具，不发送未保存草稿。`}
            >
              本轮上下文：
              {active ? `「${active.title}」已保存版本` : "未选择模块"} ·
              工具范围：当前工作区 · 不发送未保存草稿
            </p>
            <textarea
              ref={inputRef}
              aria-label="发送给 Agent 的消息"
              value={input}
              rows={1}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (
                  e.key === "Enter" &&
                  !e.shiftKey &&
                  !e.nativeEvent.isComposing
                ) {
                  e.preventDefault();
                  void send();
                }
              }}
              placeholder="描述一个目标，或者试试「创建一个本周计划」…"
              disabled={activeRun || sending}
            />
            <div className="composer-tools">
              <span className="input-hint">
                {activeRun ? (
                  <>
                    <LoaderCircle size={13} className="spin" />
                    思考中
                  </>
                ) : pending ? (
                  <button onClick={() => setPanel("review")}>
                    {pending} 项待审批
                  </button>
                ) : (
                  <>
                    <kbd>↵</kbd> 发送
                  </>
                )}
              </span>
              {activeRun ? (
                <button
                  className="send-button stop"
                  aria-label="取消当前任务"
                  onClick={() => void uiDispatch({ type: "cancel" })}
                >
                  <Square size={14} fill="currentColor" />
                </button>
              ) : (
                <button
                  className="send-button"
                  aria-label="发送消息"
                  disabled={
                    !input.trim() || sending || pending > 0 || !!agentBlock
                  }
                  onClick={() => void send()}
                >
                  <ArrowRight size={20} />
                </button>
              )}
            </div>
          </div>
          <div className="composer-status">
            <span
              className={
                "status-square " +
                (pending
                  ? "waiting_approval"
                  : activeRun
                    ? "running"
                    : snapshot.runStatus === "failed"
                      ? "error"
                      : "idle")
              }
            />
            <span>
              {runLabels[snapshot.runStatus]}
              <small>
                {policyLabels[policy.effective]} ·{" "}
                {isDesktop ? "已连接本地 Host" : "交互预览"}
              </small>
            </span>
          </div>
        </footer>
        {error && (
          <div className="error-toast" role="alert">
            <span>
              <strong>操作未完成</strong>
              {error}
            </span>
            <button
              aria-label="关闭错误提示"
              className="icon-button"
              onClick={() => setError("")}
            >
              <X size={17} />
            </button>
          </div>
        )}
        {panel === "create" && (
          <CreatePanel dispatch={dispatch} onClose={() => setPanel(null)} />
        )}
        {panel === "review" && (
          <ReviewPanel
            snapshot={snapshot}
            dispatch={dispatch}
            onClose={() => setPanel(null)}
          />
        )}
        {panel === "settings" && (
          <SettingsPanel
            appInfo={appInfo}
            snapshot={snapshot}
            dispatch={dispatch}
            onClose={() => setPanel(null)}
            onChoose={() => void chooseWorkspace()}
          />
        )}
        {panel === "policy" && (
          <PolicyPanel
            snapshot={snapshot}
            dispatch={dispatch}
            onClose={() => setPanel(null)}
            onSettings={() => setPanel("settings")}
          />
        )}
        {inspect && snapshot.modules.find((m) => m.id === inspect) && (
          <InspectPanel
            module={snapshot.modules.find((m) => m.id === inspect)!}
            snapshot={snapshot}
            dispatch={dispatch}
            onClose={() => setInspect(null)}
          />
        )}
        {panel === "command" && (
          <Dialog title="快速查找" onClose={() => setPanel(null)}>
            <div className="command-search">
              <Search size={19} />
              <input
                autoFocus
                placeholder="搜索模块，或输入一个动作…"
                aria-label="搜索模块或操作"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>
            <div className="command-results">
              {[
                {
                  label: "添加新模块",
                  icon: Plus,
                  action: () => setPanel("create"),
                },
                {
                  label: "查看运行记录",
                  icon: Activity,
                  action: () => {
                    setView("events");
                    setFocused(null);
                    setPanel(null);
                  },
                },
                {
                  label: "演示任务失败",
                  icon: Zap,
                  action: () => {
                    setPanel(null);
                    prompt("演示一次任务失败");
                  },
                },
                {
                  label: "演示长任务与取消",
                  icon: ArrowDown,
                  action: () => {
                    setPanel(null);
                    prompt("运行一个长任务");
                  },
                },
                {
                  label: "工作区设置",
                  icon: Settings2,
                  action: () => setPanel("settings"),
                },
              ]
                .filter((a) => !search || a.label.includes(search))
                .map((a) => (
                  <button key={a.label} onClick={a.action}>
                    <a.icon size={17} />
                    {a.label}
                    <ArrowRight size={14} />
                  </button>
                ))}
              {snapshot.modules
                .filter(
                  (m) =>
                    !search ||
                    m.title.includes(search) ||
                    m.type.includes(search),
                )
                .map((m) => (
                  <button
                    key={m.id}
                    onClick={() => {
                      setPanel(null);
                      focusModule(m.id);
                    }}
                  >
                    <FileText size={17} />
                    {m.title}
                    <span>聚焦模块</span>
                  </button>
                ))}
            </div>
          </Dialog>
        )}
      </div>
      {switching && (
        <div className="workspace-switch-status" role="status">
          正在切换工作区…
        </div>
      )}
    </>
  );
}
