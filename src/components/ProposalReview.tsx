import type { Approval, ModuleProposal, WorkspaceModule } from "../lib/types";
import { proposalStatusLabels as sharedProposalStatusLabels } from "../lib/ui-copy";

export const proposalStatusLabels = sharedProposalStatusLabels;
function parseModule(value: string | null): WorkspaceModule | null {
  try {
    const parsed: unknown = JSON.parse(value || "null");
    return parsed && typeof parsed === "object" && "type" in parsed
      ? (parsed as WorkspaceModule)
      : null;
  } catch {
    return null;
  }
}
function printable(value: unknown) {
  return JSON.stringify(value, null, 2);
}
export function ProposalDiff({ approval }: { approval: Approval }) {
  const before = parseModule(approval.before),
    after = parseModule(approval.after);
  if (!after)
    return (
      <div className="diff">
        <section>
          <h4>原内容</h4>
          <pre>{approval.before}</pre>
        </section>
        <section>
          <h4>提议内容</h4>
          <pre>{approval.after}</pre>
        </section>
      </div>
    );
  const taskRows = [
    ...new Set([
      ...(before?.tasks ?? []).map((t) => t.id),
      ...after.tasks.map((t) => t.id),
    ]),
  ]
    .map((id) => {
      const old = before?.tasks.find((t) => t.id === id),
        next = after.tasks.find((t) => t.id === id);
      if (printable(old) === printable(next)) return null;
      const describe = (task: typeof old) =>
        task
          ? `${task.done ? "已完成" : "未完成"} · ${task.title}${task.time ? ` · ${task.time}` : ""}${task.tag ? ` · ${task.tag}` : ""}`
          : "—";
      return (
        <div className="proposal-task-change" key={id}>
          <strong>{!next ? "移除任务" : !old ? "新增任务" : "修改任务"}</strong>
          <span>{describe(old)}</span>
          <span>→ {describe(next)}</span>
        </div>
      );
    })
    .filter(Boolean);
  return (
    <div className="module-proposal-diff">
      <p className="inline-warning">等待审批：尚未写入</p>
      <dl className="metadata">
        <dt>目标模块</dt>
        <dd>
          {after.title} · {after.type}
        </dd>
        <dt>操作</dt>
        <dd>{before ? "更新已有模块" : "创建模块"}</dd>
        <dt>基于版本</dt>
        <dd>{approval.revision || before?.moduleRevision || "新模块"}</dd>
        {before?.title !== after.title && (
          <>
            <dt>标题</dt>
            <dd>
              {before?.title || "—"} → {after.title}
            </dd>
          </>
        )}
        {printable(before?.layout) !== printable(after.layout) && (
          <>
            <dt>位置与尺寸</dt>
            <dd>
              {before
                ? `(${before.layout.x}, ${before.layout.y}) ${before.layout.w} × ${before.layout.h}`
                : "新增"}{" "}
              → ({after.layout.x}, {after.layout.y}) {after.layout.w} ×{" "}
              {after.layout.h}
            </dd>
          </>
        )}
      </dl>
      {!!taskRows.length && (
        <section aria-label="任务变更">
          <h4>任务差异（含移除）</h4>
          {taskRows}
        </section>
      )}
      <details className="proposal-validation-details">
        <summary>查看校验规则</summary>
        <p className="help-text">
          应用前会复核模块版本、当前权限、尺寸和布局碰撞。
        </p>
      </details>
      {before?.content !== after.content && after.type === "document" && (
        <div className="diff">
          <section>
            <h4>原文</h4>
            <pre>{before?.content || "空文档"}</pre>
          </section>
          <section>
            <h4>修改后</h4>
            <pre>{after.content}</pre>
          </section>
        </div>
      )}
      {printable(before?.dashboardConfig) !==
        printable(after.dashboardConfig) &&
        after.type === "dashboard" && (
          <div className="diff">
            <section>
              <h4>原指标 / 筛选</h4>
              <pre>{printable(before?.dashboardConfig) || "默认配置"}</pre>
            </section>
            <section>
              <h4>提议指标 / 筛选</h4>
              <pre>{printable(after.dashboardConfig) || "默认配置"}</pre>
            </section>
          </div>
        )}
    </div>
  );
}
export function ProposalHistory({
  proposals,
}: {
  proposals: ModuleProposal[];
}) {
  if (!proposals.length) return null;
  return (
    <section className="proposal-history" aria-label="模块提案结果">
      <h4>最近模块提案</h4>
      {proposals
        .slice(-10)
        .reverse()
        .map((proposal) => (
          <article key={proposal.id} data-proposal-status={proposal.status}>
            <div>
              <strong>{proposal.title}</strong>
              <span>
                {proposalStatusLabels[proposal.status] ?? proposal.status}
              </span>
            </div>
            <p>{proposal.summary}</p>
            {proposal.resultRevision && (
              <small>结果版本：{proposal.resultRevision}</small>
            )}
          </article>
        ))}
    </section>
  );
}
