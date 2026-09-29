import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import ReactMarkdown from "react-markdown";
import {
  ArrowDownRight,
  ArrowRight,
  Check,
  CheckCheck,
  ChevronRight,
  Circle,
  Clock3,
  FilePenLine,
  FileText,
  Info,
  LoaderCircle,
  Plus,
  Save,
  Sparkles,
  SquareTerminal,
  Target,
  X,
} from "lucide-react";
import type {
  WorkspaceAction,
  WorkspaceModule,
  WorkspaceSnapshot,
} from "../lib/types";
import { useDocumentDraft } from "../lib/document-drafts";
import { limitTaskTitle } from "../lib/task-title";
import "./module-content.css";

export interface ModuleContentProps {
  module: WorkspaceModule;
  snapshot: WorkspaceSnapshot;
  dispatch: (action: WorkspaceAction) => Promise<void>;
  onPrompt: (text: string, moduleId?: string) => void;
  focused: boolean;
}

function formatTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "刚刚";
  return date.toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function PixelMark({ small = false }: { small?: boolean }) {
  return (
    <span
      className={`mc-pixel-mark${small ? " mc-pixel-mark-small" : ""}`}
      aria-hidden="true"
    >
      <i />
      <i />
      <i />
      <i />
      <i />
      <i />
      <i />
      <i />
      <i />
    </span>
  );
}

function Conversation({ module, snapshot, onPrompt }: ModuleContentProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const shouldScroll = useRef(true);
  const latestText = snapshot.messages.at(-1)?.text;
  const running = snapshot.runStatus === "running";
  useEffect(() => {
    if (
      (snapshot.messages.length > 1 || running) &&
      shouldScroll.current &&
      scrollRef.current
    )
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
  }, [latestText, snapshot.messages.length, running]);
  const quickActions = [
    {
      icon: Target,
      label: "规划这一周",
      sub: "把想法变成可执行的计划",
      prompt: "帮我创建一个本周计划，安排三项重要任务。",
    },
    {
      icon: FilePenLine,
      label: "一起写点什么",
      sub: "梳理、起草，或者修改文档",
      prompt: "帮我创建一份项目说明文档。",
    },
  ];
  return (
    <div
      className="mc-conversation mc-scroll"
      ref={scrollRef}
      onScroll={() => {
        const el = scrollRef.current;
        if (el)
          shouldScroll.current =
            el.scrollHeight - el.scrollTop - el.clientHeight < 100;
      }}
    >
      <div className="mc-welcome">
        <div className="mc-welcome-top">
          <PixelMark />
        </div>
        <h2>说出目标，Agent 会帮你整理。</h2>
        {snapshot.messages.length === 0 && (
          <div className="mc-quick-actions">
            {quickActions.map(({ icon: Icon, label, sub, prompt }) => (
              <button
                key={label}
                className="mc-quick-action"
                onClick={() => onPrompt(prompt, module.id)}
              >
                <Icon size={17} strokeWidth={1.8} />
                <span>
                  <strong>{label}</strong>
                  <small>{sub}</small>
                </span>
                <ArrowDownRight size={17} />
              </button>
            ))}
          </div>
        )}
      </div>
      {snapshot.messages.length > 0 && (
        <div
          className="mc-chat-list"
          role="log"
          aria-label="Agent 对话记录"
          aria-live="polite"
          aria-relevant="additions text"
        >
          {snapshot.messages.map((message, index) => (
            <article
              className={`mc-message mc-message-${message.role}`}
              key={message.id}
            >
              <div className="mc-message-meta">
                {message.role === "assistant" ? (
                  <PixelMark small />
                ) : (
                  <span className="mc-user-avatar">我</span>
                )}
                <strong>
                  {message.role === "assistant" ? "工作空间助手" : "你"}
                </strong>
                <time dateTime={message.timestamp}>
                  {formatTime(message.timestamp)}
                </time>
              </div>
              <div className="mc-message-text">
                <ReactMarkdown
                  skipHtml
                  components={{
                    img: ({ alt }) => <span>[图片：{alt || "图片"}]</span>,
                    a: ({ href, children }) => (
                      <a href={href} target="_blank" rel="noreferrer noopener">
                        {children}
                      </a>
                    ),
                  }}
                >
                  {message.text}
                </ReactMarkdown>
                {running &&
                  message.role === "assistant" &&
                  index === snapshot.messages.length - 1 && (
                    <span className="mc-stream-cursor" aria-label="正在生成" />
                  )}
              </div>
            </article>
          ))}
          {running && snapshot.messages.at(-1)?.role !== "assistant" && (
            <div className="mc-thinking">
              <PixelMark small />
              <span>正在整理思路</span>
              <span className="mc-thinking-dots">•••</span>
            </div>
          )}
        </div>
      )}
      {!running && (
        <div className="mc-conversation-note">
          <SquareTerminal size={13} />
          <span>在下方输入，开始一次协作</span>
          <ArrowDownRight size={13} />
        </div>
      )}
    </div>
  );
}

function Planner({ module, dispatch, onPrompt }: ModuleContentProps) {
  const [adding, setAdding] = useState(false);
  const [title, setTitle] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const done = module.tasks.filter((task) => task.done).length;
  const progress = module.tasks.length
    ? Math.round((done / module.tasks.length) * 100)
    : 0;
  async function addTask(event: FormEvent) {
    event.preventDefault();
    if (!title.trim() || saving) return;
    setSaving(true);
    setError("");
    try {
      await dispatch({
        type: "add_task",
        moduleId: module.id,
        title: title.trim(),
      });
      setTitle("");
      setAdding(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : "添加失败，请重试。");
    } finally {
      setSaving(false);
    }
  }
  async function toggle(taskId: string) {
    setError("");
    try {
      await dispatch({ type: "toggle_task", moduleId: module.id, taskId });
    } catch (err) {
      setError(err instanceof Error ? err.message : "更新失败，请重试。");
    }
  }
  return (
    <div className="mc-planner mc-scroll">
      <div className="mc-plan-summary">
        <div>
          <span className="mc-eyebrow">MAKE A LITTLE PROGRESS</span>
          <h3>专注当下，逐一完成。</h3>
        </div>
        <span className="mc-progress-count">
          {done}
          <span> / {module.tasks.length}</span>
        </span>
      </div>
      <div
        className="mc-progress-bar"
        role="progressbar"
        aria-label="任务完成进度"
        aria-valuenow={progress}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <span style={{ width: `${progress}%` }} />
      </div>
      <div className="mc-task-section-label">
        <span>待办事项</span>
        <span>{progress}% 已完成</span>
      </div>
      <div className="mc-task-list">
        {module.tasks.map((task, index) => (
          <div
            className={`mc-task${task.done ? " mc-task-done" : ""}`}
            key={task.id}
          >
            <button
              className="mc-task-checkbox"
              role="checkbox"
              aria-checked={task.done}
              aria-label={`${task.done ? "标记未完成" : "完成任务"}：${task.title}`}
              onClick={() => void toggle(task.id)}
            >
              {task.done && <Check size={12} strokeWidth={3} />}
            </button>
            <div className="mc-task-main">
              <span className="mc-task-title">{task.title}</span>
              <div className="mc-task-details">
                {task.time && (
                  <span>
                    <Clock3 size={10} />
                    {task.time}
                  </span>
                )}
                {task.tag && (
                  <span className={`mc-task-tag mc-task-tag-${index % 3}`}>
                    {task.tag}
                  </span>
                )}
              </div>
            </div>
          </div>
        ))}
        {module.tasks.length === 0 && (
          <div className="mc-empty-plan">
            <CheckCheck size={26} />
            <p>暂无任务</p>
            <span>添加任务，或让 Agent 帮你规划。</span>
          </div>
        )}
      </div>
      {adding ? (
        <form className="mc-add-task-form" onSubmit={addTask}>
          <label className="mc-sr-only" htmlFor={`task-title-${module.id}`}>
            新任务名称
          </label>
          <input
            id={`task-title-${module.id}`}
            autoFocus
            placeholder="下一件重要的事……"
            value={title}
            onChange={(event) => setTitle(limitTaskTitle(event.target.value))}
            disabled={saving}
          />
          <button
            type="submit"
            className="mc-icon-button mc-save-task"
            aria-label="保存新任务"
            disabled={!title.trim() || saving}
          >
            {saving ? (
              <LoaderCircle size={15} className="mc-spin" />
            ) : (
              <Check size={15} />
            )}
          </button>
          <button
            type="button"
            className="mc-icon-button"
            aria-label="取消添加任务"
            onClick={() => {
              setAdding(false);
              setTitle("");
            }}
            disabled={saving}
          >
            <X size={15} />
          </button>
        </form>
      ) : (
        <button className="mc-add-task" onClick={() => setAdding(true)}>
          <Plus size={14} />
          添加任务<span className="mc-key-hint">+</span>
        </button>
      )}
      {error && (
        <p className="mc-inline-error" role="alert">
          {error}
        </p>
      )}
      <button
        className="mc-text-prompt"
        onClick={() =>
          onPrompt("根据当前目标，创建一个新的本周计划。", module.id)
        }
      >
        <Sparkles size={13} />
        生成新的计划提案
        <ArrowRight size={13} />
      </button>
    </div>
  );
}

function Document({
  module,
  snapshot,
  dispatch,
  onPrompt,
}: ModuleContentProps) {
  const [cached, updateDraft] = useDocumentDraft(snapshot.rootPath, module.id);
  const editing = !!cached;
  const draft = cached?.text ?? module.content;
  const baseRevision = cached ? cached.baseRevision : module.revision;
  const saving = cached?.saving ?? false;
  const submitted = !!cached && cached.submitted !== null;
  const error = cached?.error ?? "";
  const recoveryKey = useRef<string | null>(null);
  const draftKey = JSON.stringify([snapshot.rootPath, module.id]);
  const pendingApproval = snapshot.approvals.some(
    (approval) =>
      approval.kind === "write_file" && approval.moduleId === module.id,
  );
  useEffect(() => {
    const restoring = recoveryKey.current !== draftKey;
    // A restored page can arrive before its old dispatch promise settles.
    // Keep recovery pending until that request has finished, so an approval
    // handled while this component was absent can still unlock its draft.
    if (!cached?.saving) recoveryKey.current = draftKey;
    if (!cached || cached.submitted === null) return;
    if (
      !pendingApproval &&
      module.content === cached.submitted &&
      module.revision !== cached.baseRevision
    ) {
      updateDraft(undefined);
    } else if (pendingApproval && !cached.hadApproval) {
      updateDraft({ ...cached, hadApproval: true });
    } else if (
      !pendingApproval &&
      !cached.saving &&
      (cached.hadApproval || restoring)
    ) {
      updateDraft({
        ...cached,
        submitted: null,
        hadApproval: false,
        error: "写入未通过，草稿已保留。",
      });
    }
  }, [
    pendingApproval,
    module.content,
    module.revision,
    cached,
    draftKey,
    updateDraft,
  ]);
  function startEditing() {
    updateDraft({
      text: module.content,
      baseRevision: module.revision,
      submitted: null,
      saving: false,
      hadApproval: false,
      error: "",
    });
  }
  async function save() {
    if (
      !cached ||
      saving ||
      submitted ||
      pendingApproval ||
      draft === module.content
    )
      return;
    updateDraft({ ...cached, saving: true, error: "", submitted: draft });
    try {
      await dispatch({
        type: "edit_document",
        moduleId: module.id,
        content: draft,
        revision: baseRevision,
      });
    } catch (err) {
      updateDraft((current) =>
        current
          ? {
              ...current,
              submitted: null,
              error: err instanceof Error ? err.message : String(err),
            }
          : current,
      );
    } finally {
      updateDraft((current) =>
        current ? { ...current, saving: false } : current,
      );
    }
  }
  return (
    <div className={`mc-document${editing ? " mc-document-editing" : ""}`}>
      <div className="mc-document-toolbar">
        <span className="mc-file-indicator">
          <FileText size={12} />
          <span title={module.filePath || "工作空间文档"}>
            {module.filePath?.split(/[\\/]/).at(-1) || "未命名.md"}
          </span>
        </span>
        <div className="mc-document-tools">
          {editing ? (
            <>
              <button
                className="mc-toolbar-button"
                disabled={saving || submitted || pendingApproval}
                onClick={() => updateDraft(undefined)}
              >
                <X size={12} />
                取消
              </button>
              <button
                className="mc-toolbar-button mc-toolbar-primary"
                onClick={() => void save()}
                disabled={
                  saving ||
                  submitted ||
                  pendingApproval ||
                  draft === module.content
                }
              >
                {saving ? (
                  <LoaderCircle size={12} className="mc-spin" />
                ) : (
                  <Save size={12} />
                )}
                {saving ? "提交中" : "保存"}
              </button>
            </>
          ) : (
            <button
              className="mc-toolbar-button"
              disabled={pendingApproval}
              onClick={startEditing}
            >
              <FilePenLine size={12} />
              编辑
            </button>
          )}
        </div>
      </div>
      {pendingApproval && (
        <div className="mc-document-notice" role="status">
          <Clock3 size={13} />
          修改已提交，等待确认写入。
        </div>
      )}
      {error && (
        <div className="mc-inline-error mc-document-error" role="alert">
          {error}
        </div>
      )}
      {editing ? (
        <>
          <label
            className="mc-sr-only"
            htmlFor={`document-editor-${module.id}`}
          >
            编辑 Markdown 文档
          </label>
          <textarea
            id={`document-editor-${module.id}`}
            className="mc-document-editor"
            aria-label="编辑 Markdown 文档"
            value={draft}
            onChange={(event) =>
              updateDraft((current) =>
                current ? { ...current, text: event.target.value } : current,
              )
            }
            disabled={saving || submitted || pendingApproval}
            spellCheck={false}
          />
          <div className="mc-editor-footnote">
            <span>MARKDOWN</span>
            <span>{draft.length.toLocaleString()} 字符 · 草稿</span>
          </div>
        </>
      ) : (
        <div className="mc-markdown mc-scroll">
          <ReactMarkdown
            skipHtml
            components={{
              img: ({ alt }) => (
                <span className="mc-image-placeholder">
                  [图片：{alt || "图片"}]
                </span>
              ),
              a: ({ href, children }) => (
                <a href={href} target="_blank" rel="noreferrer noopener">
                  {children}
                </a>
              ),
            }}
          >
            {module.content || "# 空白文档"}
          </ReactMarkdown>
        </div>
      )}
      {!editing && (
        <button
          className="mc-document-prompt"
          onClick={() =>
            onPrompt("请帮我修改这份文档的结构和表达，保留原意。", module.id)
          }
        >
          <Sparkles size={13} />
          <span>让 Agent 一起编辑</span>
          <ArrowRight size={13} />
        </button>
      )}
    </div>
  );
}

function Dashboard({ module, snapshot }: ModuleContentProps) {
  const config = module.dashboardConfig;
  const planners = snapshot.modules.filter(
    (m) =>
      m.type === "planner" &&
      (!config?.plannerIds.length || config.plannerIds.includes(m.id)),
  );
  const tasks = planners.flatMap((m) => m.tasks);
  const completed = tasks.filter((task) => task.done).length;
  const percentage = tasks.length
    ? Math.round((completed / tasks.length) * 100)
    : 0;
  const events = snapshot.events.slice(-3).reverse();
  const metrics = config?.metrics ?? ["tasks_done", "module_count"];
  const metricValues = {
    tasks_done: {
      label: "任务完成",
      value: completed,
      detail: `${percentage}% 完成率 · 共 ${tasks.length} 项`,
    },
    tasks_total: {
      label: "任务总数",
      value: tasks.length,
      detail: `筛选范围内 ${planners.length} 个计划`,
    },
    module_count: {
      label: "工作模块",
      value: snapshot.modules.length,
      detail: "当前工作区全部模块",
    },
    document_count: {
      label: "文档数量",
      value: snapshot.modules.filter((m) => m.type === "document").length,
      detail: "当前工作区已保存文档",
    },
  };
  return (
    <div className="mc-dashboard mc-scroll">
      <div className="mc-dashboard-intro">
        <span className="mc-eyebrow">A CLEARER PICTURE</span>
        <span className="mc-live-label">
          <i />
          实时状态
        </span>
      </div>
      {config?.title && <h3>{config.title}</h3>}
      <span
        className="mc-dashboard-filter"
        role="img"
        aria-label={`数据来源：当前工作区，${config?.plannerIds.length ? `已选 ${planners.length} 个计划` : "全部计划"}`}
      >
        <Info size={12} aria-hidden="true" />
        数据来源
      </span>
      <div className="mc-metrics">
        {metrics.map((metric) => {
          const item = metricValues[metric];
          return item ? (
            <div className="mc-metric" key={metric} data-metric={metric}>
              <span>{item.label}</span>
              <div>
                <strong>{item.value}</strong>
              </div>
              <span className="mc-metric-note">{item.detail}</span>
            </div>
          ) : null;
        })}
      </div>
      <div className="mc-activity-heading">
        <span>任务进度</span>
        <span>
          {completed} / {tasks.length}
        </span>
      </div>
      <div
        className="mc-progress-bar"
        role="progressbar"
        aria-label="看板真实任务完成率"
        aria-valuenow={percentage}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <span style={{ width: `${percentage}%` }} />
      </div>
      <div className="mc-events-heading">
        <span>最近动态</span>
        <span className="mc-eyebrow">ACTIVITY LOG</span>
      </div>
      <div className="mc-events">
        {events.length ? (
          events.map((event) => {
            const failure = /error|failed|reject|denied/i.test(event.kind);
            const pending = /approval|request/i.test(event.kind);
            return (
              <div className="mc-event" key={event.seq}>
                <span
                  className={`mc-event-dot${failure ? " mc-event-dot-error" : pending ? " mc-event-dot-pending" : ""}`}
                >
                  {failure ? (
                    <X size={9} />
                  ) : pending ? (
                    <Circle size={8} />
                  ) : (
                    <Check size={9} />
                  )}
                </span>
                <span className="mc-event-message" title={event.message}>
                  {event.message}
                </span>
                <time dateTime={event.timestamp}>
                  {formatTime(event.timestamp)}
                </time>
              </div>
            );
          })
        ) : (
          <div className="mc-events-empty">
            <Circle size={11} />
            <span>工作空间已准备好，等待你的第一步。</span>
          </div>
        )}
      </div>
      <div className="mc-dashboard-foot">
        <span>
          {snapshot.agent.transport === "mock"
            ? "MOCK AGENT"
            : snapshot.agent.name.toUpperCase()}
        </span>
        <span>
          {snapshot.events.length} 条事件
          <ChevronRight size={11} />
        </span>
      </div>
    </div>
  );
}

export function ModuleContent(props: ModuleContentProps) {
  return (
    <div
      className={`mc-root mc-type-${props.module.type}${props.focused ? " mc-focused" : ""}`}
    >
      {props.module.type === "conversation" && <Conversation {...props} />}
      {props.module.type === "planner" && <Planner {...props} />}
      {props.module.type === "document" && <Document {...props} />}
      {props.module.type === "dashboard" && <Dashboard {...props} />}
    </div>
  );
}
