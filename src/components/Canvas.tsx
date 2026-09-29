import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import {
  Copy,
  Grip,
  Maximize2,
  Minimize2,
  MoreHorizontal,
  SlidersHorizontal,
  X,
} from "lucide-react";
import type {
  Layout,
  WorkspaceAction,
  WorkspaceModule,
  WorkspaceSnapshot,
} from "../lib/types";
import { COLS, GAP, ROW, clampLayout, moveModule } from "../lib/layout";
import { ModuleContent } from "./ModuleContent";
import { ModuleErrorBoundary } from "./ModuleErrorBoundary";
const names = {
  conversation: "对话",
  planner: "计划",
  document: "文档",
  dashboard: "看板",
};
const statuses = {
  idle: "就绪",
  running: "运行中",
  waiting_approval: "待审批",
  attention: "已更新",
  error: "错误",
  offline: "离线",
};
interface Props {
  snapshot: WorkspaceSnapshot;
  selected: string | null;
  onSelect: (id: string) => void;
  focused: string | null;
  onFocus: (id: string | null) => void;
  dispatch: (a: WorkspaceAction) => Promise<void>;
  contentDispatch: (a: WorkspaceAction) => Promise<void>;
  onPrompt: (text: string, id?: string) => void;
  onInspect: (id: string) => void;
}
export function Canvas({
  snapshot,
  selected,
  onSelect,
  focused,
  onFocus,
  dispatch,
  contentDispatch,
  onPrompt,
  onInspect,
}: Props) {
  const latest = useRef(snapshot);
  latest.current = snapshot;
  const ref = useRef<HTMLDivElement>(null),
    scroll = useRef(0),
    lastFocus = useRef<string | null>(null);
  const [width, setWidth] = useState(1000),
    [draft, setDraft] = useState<{ id: string; layout: Layout } | null>(null),
    [menu, setMenu] = useState<string | null>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) =>
      setWidth(entries[0].contentRect.width),
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (focused && !lastFocus.current) {
      scroll.current = ref.current?.parentElement?.scrollTop || 0;
    } else if (!focused && lastFocus.current) {
      ref.current?.parentElement?.scrollTo({ top: scroll.current });
    }
    const focusId = focused || lastFocus.current;
    if (focusId)
      requestAnimationFrame(() => {
        const el = ref.current?.querySelector<HTMLElement>(
          `[data-module-id="${focusId}"]`,
        );
        el?.focus({ preventScroll: true });
      });
    lastFocus.current = focused;
  }, [focused]);
  function start(e: PointerEvent, m: WorkspaceModule, resize = false) {
    if (
      focused ||
      e.button !== 0 ||
      (e.target as HTMLElement).closest("button,input,textarea")
    )
      return;
    e.preventDefault();
    onSelect(m.id);
    const startX = e.clientX,
      startY = e.clientY,
      startLayout = { ...m.layout },
      startScroll = ref.current?.parentElement?.scrollTop || 0,
      startTime = performance.now(),
      isTouch = e.pointerType === "touch";
    let current = startLayout,
      changed = false;
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const move = (ev: globalThis.PointerEvent) => {
      if (isTouch && performance.now() - startTime < 300) return;
      const dx = Math.round((ev.clientX - startX) / (width / COLS)),
        dy = Math.round(
          (ev.clientY -
            startY +
            (ref.current?.parentElement?.scrollTop || 0) -
            startScroll) /
            ROW,
        );
      if (!dx && !dy && !changed) return;
      changed = true;
      current = clampLayout(
        resize
          ? { ...startLayout, w: startLayout.w + dx, h: startLayout.h + dy }
          : { ...startLayout, x: startLayout.x + dx, y: startLayout.y + dy },
      );
      setDraft({ id: m.id, layout: current });
      const scroller = ref.current?.parentElement;
      if (scroller) {
        const b = scroller.getBoundingClientRect();
        if (ev.clientY > b.bottom - 42) scroller.scrollBy(0, 12);
        if (ev.clientY < b.top + 42) scroller.scrollBy(0, -12);
      }
    };
    const finish = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", finish);
      el.removeEventListener("pointercancel", cancel);
      if (changed)
        void dispatch({
          type: "set_layouts",
          layouts: moveModule(
            latest.current.modules,
            m.id,
            current,
            latest.current.allowOverlap,
          ),
        });
      setDraft(null);
    };
    const cancel = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", finish);
      el.removeEventListener("pointercancel", cancel);
      setDraft(null);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", finish);
    el.addEventListener("pointercancel", cancel);
  }
  function keyboard(e: KeyboardEvent, m: WorkspaceModule) {
    if (e.target !== e.currentTarget || focused) return;
    if (e.key === "Enter") {
      e.preventDefault();
      onFocus(m.id);
      return;
    }
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key))
      return;
    e.preventDefault();
    let l = { ...m.layout };
    const dx = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0,
      dy = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (e.shiftKey) {
      l.w += dx;
      l.h += dy;
    } else {
      l.x += dx;
      l.y += dy;
    }
    void dispatch({
      type: "set_layouts",
      layouts: moveModule(snapshot.modules, m.id, l, snapshot.allowOverlap),
    });
  }
  const height = Math.max(
    640,
    ...snapshot.modules.map((m) => (m.layout.y + m.layout.h) * ROW + 64),
    draft ? (draft.layout.y + draft.layout.h) * ROW + 64 : 0,
  );
  return (
    <div
      className={"canvas-scroll" + (focused ? " in-focus" : "")}
      onClick={() => menu && setMenu(null)}
    >
      <div ref={ref} className="canvas" style={{ height }} data-testid="canvas">
        {snapshot.modules.map((m, i) => {
          const l = draft?.id === m.id ? draft.layout : m.layout;
          return (
            <article
              key={m.id}
              data-testid={"module-" + m.type}
              data-module-id={m.id}
              tabIndex={focused && focused !== m.id ? -1 : 0}
              aria-label={m.title + "模块"}
              onFocus={() => onSelect(m.id)}
              onKeyDown={(e) => keyboard(e, m)}
              onPointerDown={() => onSelect(m.id)}
              className={
                "workspace-module " +
                (selected === m.id ? "selected " : "") +
                (focused === m.id ? "focused " : "") +
                (focused && focused !== m.id ? "concealed " : "") +
                (draft?.id === m.id ? "dragging" : "")
              }
              style={
                focused === m.id
                  ? undefined
                  : {
                      left: (l.x / COLS) * width,
                      top: l.y * ROW,
                      width: (l.w / COLS) * width - GAP,
                      height: l.h * ROW - GAP,
                      zIndex:
                        draft?.id === m.id ? 20 : selected === m.id ? 3 : 1,
                    }
              }
            >
              <header
                className="module-header"
                onPointerDown={(e) => start(e, m)}
                onDoubleClick={(e) => {
                  if (!(e.target as HTMLElement).closest("button"))
                    onFocus(focused === m.id ? null : m.id);
                }}
              >
                <span className="module-index">
                  {String(i + 1).padStart(2, "0")}
                </span>
                <span className={"status-square " + m.status} />
                <h2 title={m.title}>{m.title}</h2>
                <span className="module-kind">{names[m.type]}</span>
                <button
                  className="icon-button"
                  title={
                    focused === m.id ? "返回画布 · Esc" : "聚焦模块 · 双击"
                  }
                  aria-label={focused === m.id ? "返回画布" : "聚焦" + m.title}
                  onClick={() => onFocus(focused === m.id ? null : m.id)}
                >
                  {focused === m.id ? (
                    <Minimize2 size={15} />
                  ) : (
                    <Maximize2 size={15} />
                  )}
                </button>
                <button
                  className="icon-button"
                  aria-label={m.title + "更多操作"}
                  onClick={(e) => {
                    e.stopPropagation();
                    setMenu(menu === m.id ? null : m.id);
                  }}
                >
                  <MoreHorizontal size={18} />
                </button>
                {menu === m.id && (
                  <div
                    className="module-menu"
                    onPointerDown={(e) => e.stopPropagation()}
                  >
                    <button
                      onClick={() => {
                        onInspect(m.id);
                        setMenu(null);
                      }}
                    >
                      <SlidersHorizontal size={14} />
                      模块设置
                    </button>
                    <button
                      onClick={() => {
                        void dispatch({
                          type: "duplicate_module",
                          moduleId: m.id,
                        });
                        setMenu(null);
                      }}
                    >
                      <Copy size={14} />
                      复制模块
                    </button>
                    <button
                      className="danger-text"
                      onClick={() => {
                        void dispatch({ type: "close_module", moduleId: m.id });
                        if (focused === m.id) onFocus(null);
                        setMenu(null);
                      }}
                    >
                      <X size={14} />
                      移出画布
                    </button>
                  </div>
                )}
              </header>
              <div className="module-body">
                <ModuleErrorBoundary
                  moduleName={m.title}
                  resetKey={m.revision ?? m.id}
                >
                  <ModuleContent
                    module={m}
                    snapshot={snapshot}
                    dispatch={contentDispatch}
                    onPrompt={onPrompt}
                    focused={focused === m.id}
                  />
                </ModuleErrorBoundary>
              </div>
              <footer className="module-footer">
                <span>
                  <i className={"tiny-dot " + m.status} />
                  {statuses[m.status]}
                </span>
                <span>
                  {focused === m.id
                    ? "ESC 返回画布"
                    : draft?.id === m.id
                      ? `X ${l.x} · Y ${l.y} · ${l.w} × ${l.h}`
                      : "双击聚焦"}
                </span>
              </footer>
              {!focused && (
                <div
                  role="separator"
                  aria-label={"调整" + m.title + "大小"}
                  className="resize-handle"
                  onPointerDown={(e) => start(e, m, true)}
                >
                  <Grip size={13} />
                </div>
              )}
            </article>
          );
        })}
        {snapshot.modules.length === 0 && (
          <div className="empty-board">
            <span className="pixel-mark">▦</span>
            <h2>暂无模块</h2>
            <p>添加模块，或告诉 Agent 目标。</p>
          </div>
        )}
        {draft && (
          <div className="drop-coordinates">
            {draft.layout.w} × {draft.layout.h} · 24 列网格
          </div>
        )}
      </div>
    </div>
  );
}
