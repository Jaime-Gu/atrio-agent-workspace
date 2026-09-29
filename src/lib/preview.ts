// Browser-only demonstration. Native releases use the Rust/SQLite kernel exclusively.
import type {
  Approval,
  ModuleType,
  PolicyMode,
  SystemPolicy,
  WorkspaceAction,
  WorkspaceModule,
  WorkspaceSnapshot,
  WorkspacePolicy,
  ModuleProposal,
} from "./types";
import { agentProvider, providerName } from "./providers";
const key = "pixel-workspace-preview-v1";
const uid = () => crypto.randomUUID();
const now = () => new Date().toISOString();
const doc =
  "# 把想法，放上工作台\n\n这里是你的个人工作区。对话只是起点，计划、文档和数据都可以成为独立的模块。\n\n## 今天，从一件事开始\n\n- 描述一个想要完成的目标\n- 让 Agent 创建本周计划\n- 拖动模块，安排自己的工作节奏\n\n> 双击标题栏，进入专注模式。按 Esc 回到画布。";
function moduleOf(type: ModuleType, title: string): WorkspaceModule {
  return {
    id: uid(),
    type,
    title,
    status: "idle",
    layout: { x: 0, y: 0, w: 12, h: 43 },
    tasks:
      type === "planner"
        ? [
            {
              id: uid(),
              title: "明确本周目标与优先级",
              done: false,
              time: "09:00",
              tag: "规划",
            },
            {
              id: uid(),
              title: "完成第一版内容草稿",
              done: false,
              time: "10:30",
              tag: "专注",
            },
            {
              id: uid(),
              title: "检查结果并整理下一步",
              done: false,
              time: "16:00",
              tag: "回顾",
            },
          ]
        : [],
    content: type === "document" ? doc : "",
    filePath: type === "document" ? "documents/welcome.md" : null,
    revision: type === "document" ? "preview-1" : null,
    moduleRevision: uid(),
    dashboardConfig:
      type === "dashboard"
        ? { metrics: ["tasks_done", "module_count"], plannerIds: [] }
        : null,
  };
}
function seed(): WorkspaceSnapshot {
  const a = moduleOf("conversation", "与 Agent 一起工作"),
    b = moduleOf("document", "工作区使用笔记"),
    c = moduleOf("dashboard", "工作区概览");
  a.layout = { x: 0, y: 0, w: 12, h: 47 };
  b.layout = { x: 12, y: 0, w: 12, h: 47 };
  c.layout = { x: 0, y: 47, w: 24, h: 34 };
  return {
    schemaVersion: 1,
    name: "我的工作区",
    rootPath: "浏览器预览 · 本地存储",
    workspaceId: uid(),
    providerSessionId: null,
    modules: [a, b, c],
    messages: [
      {
        id: uid(),
        role: "assistant",
        text: "你好，欢迎来到你的工作台。\n告诉我你的目标，我会把想法整理成可以继续工作的模块。\n\n试试「创建一个本周计划」。",
        timestamp: now(),
      },
    ],
    events: [
      {
        seq: 1,
        kind: "initialized",
        message: "Mock Agent 已就绪 · 预览模式",
        timestamp: now(),
        sessionId: null,
      },
    ],
    approvals: [],
    moduleProposals: [],
    runStatus: "idle",
    sessionId: null,
    permissionMode: "ask",
    policy: {
      system: "workspace",
      local: "ask",
      effective: "ask",
      source: "workspace",
      epoch: 0,
      workspaceTrusted: true,
      hermesScopeAccepted: false,
    },
    agent: {
      provider: "mock",
      id: "mock",
      name: "Mock Agent",
      transport: "mock",
      command: "",
      args: [],
      env: {},
      cwd: "workspace_root",
      probeStatus: "ready",
    },
    allowOverlap: false,
    updatedAt: now(),
  };
}
let state: WorkspaceSnapshot;
try {
  state = JSON.parse(localStorage.getItem(key) || "null") || seed();
  if (state.runStatus === "running") state.runStatus = "cancelled";
} catch {
  state = seed();
}
// A restored browser snapshot belongs to a new runtime, just like Kernel::open.
state.workspaceGeneration = uid();
state.policy ??= {
  system: "workspace",
  local: state.permissionMode,
  effective: state.permissionMode,
  source: "workspace",
  epoch: 0,
  workspaceTrusted: true,
  hermesScopeAccepted: false,
};
state.policy.hermesScopeAccepted = false;
state.policy.agentScopeAccepted = false;
state.policy.scopeProvider = null;
state.moduleProposals ??= [];
for (const module of state.modules) module.moduleRevision ??= uid();
if (state.agent.transport === "stdio")
  state.connection = {
    status: "disconnected",
    message: `真实 ${providerName(state.agent)} 仅可在原生应用中连接。`,
    providerSessionId: null,
    capabilities: null,
    hermesVersion: null,
  };
for (const approval of state.approvals) {
  approval.origin ??= "agent";
  approval.epoch ??= state.policy.epoch;
}
const listeners = new Set<(s: WorkspaceSnapshot) => void>();
let timer: ReturnType<typeof setTimeout> | undefined;
const copy = () => structuredClone(state);
function save() {
  state.updatedAt = now();
  localStorage.setItem(key, JSON.stringify(state));
  listeners.forEach((fn) => fn(copy()));
  return copy();
}
function event(kind: string, message: string) {
  state.events.push({
    seq: (state.events.at(-1)?.seq || 0) + 1,
    kind,
    message,
    timestamp: now(),
    sessionId: state.sessionId,
  });
}
function currentPolicy(): WorkspacePolicy {
  return state.policy!;
}
function updatePolicy(system: SystemPolicy, local: PolicyMode) {
  const policy = currentPolicy();
  const effective =
    system === "allow_all"
      ? "full"
      : system === "deny_all"
        ? "disabled"
        : local;
  const changed = effective !== policy.effective;
  state.policy = {
    ...policy,
    system,
    local,
    effective,
    source: system === "workspace" ? "workspace" : "system",
    epoch: policy.epoch + (changed ? 1 : 0),
    workspaceTrusted: true,
    hermesScopeAccepted: changed ? false : policy.hermesScopeAccepted,
    agentScopeAccepted: changed ? false : policy.agentScopeAccepted,
    scopeProvider: changed ? null : policy.scopeProvider,
  };
  state.permissionMode = local === "restricted" ? "restricted" : "ask";
  if (changed) {
    clearTimeout(timer);
    invalidateAgentProposals("权限变化，旧提案已取消");
    if (state.runStatus === "running" || state.runStatus === "waiting_approval")
      state.runStatus = state.approvals.length
        ? "waiting_approval"
        : "cancelled";
    event(
      "policy_changed",
      `有效权限已改为 ${effective}，旧 Agent 任务与审批已失效`,
    );
  } else event("policy_preference_updated", "已保存策略；有效权限未变化");
}
function proposalResult(
  p: Approval,
  status: ModuleProposal["status"],
  summary: string,
  revision?: string,
) {
  if (!p.proposalId) return;
  const result = state.moduleProposals!.find(
    (item) => item.id === p.proposalId,
  );
  if (result)
    Object.assign(result, {
      status,
      summary,
      resultRevision: revision,
      updatedAt: now(),
    });
}
function invalidateAgentProposals(summary: string) {
  for (const approval of state.approvals.filter((p) => p.origin !== "user"))
    proposalResult(approval, "cancelled", summary);
  state.approvals = state.approvals.filter((p) => p.origin === "user");
}
function applyProposal(p: Approval) {
  if (p.kind === "module_changes") {
    const next = JSON.parse(p.after || "null") as WorkspaceModule | null;
    if (!next) throw new Error("模块提案格式无效");
    const index = state.modules.findIndex((m) => m.id === p.moduleId);
    if (index >= 0 && state.modules[index].moduleRevision !== p.revision) {
      proposalResult(p, "conflict", "模块已被修改，请基于新版本重新提议");
      state.approvals = state.approvals.filter((item) => item.id !== p.id);
      state.runStatus = "failed";
      event("proposal_conflict", "模块版本冲突，未应用修改");
      save();
      throw new Error("模块版本冲突，请基于新版本重新生成修改");
    }
    if (p.moduleId && index < 0) throw new Error("目标模块已不存在");
    next.moduleRevision = uid();
    if (next.type === "document") next.revision = uid();
    if (index >= 0) state.modules[index] = next;
    else state.modules.push(next);
    proposalResult(
      p,
      "applied",
      "已应用到浏览器预览工作区",
      next.moduleRevision,
    );
  } else if (p.kind === "create_module") {
    const m = moduleOf(p.moduleType!, p.title);
    m.layout.y = Math.max(
      0,
      ...state.modules.map((m) => m.layout.y + m.layout.h),
    );
    state.modules.push(m);
  } else if (p.kind === "write_file") {
    const m = state.modules.find((m) => m.id === p.moduleId);
    if (m) {
      if (m.revision !== p.revision)
        throw new Error("文档已变更，请重新生成修改");
      m.content = p.after!;
      m.revision = uid();
      m.moduleRevision = uid();
    }
  }
}
function mockModuleProposal(
  text: string,
  selected: WorkspaceModule | undefined,
  creating = false,
): Approval | null {
  const type: ModuleType = /看板|dashboard/i.test(text)
    ? "dashboard"
    : /文档|document/i.test(text)
      ? "document"
      : "planner";
  const before = creating ? undefined : selected;
  if (!creating && (!before || before.type === "conversation")) return null;
  const next = before
    ? structuredClone(before)
    : moduleOf(type, `预览：${text.slice(0, 28)}`);
  let changeType: "document" | "planner" | "dashboard" | "metadata" | "create" =
    creating ? "create" : (next.type as "document" | "planner" | "dashboard");
  if (creating)
    next.layout.y = Math.max(
      0,
      ...state.modules.map((m) => m.layout.y + m.layout.h),
    );
  if (/改名|标题|rename/i.test(text)) {
    next.title = `预览：${text.replace(/^.*?(改名|标题|rename)[：:为成 ]*/i, "").slice(0, 60) || "新的模块标题"}`;
    changeType = "metadata";
  } else if (/移动|缩放|尺寸|move|resize/i.test(text)) {
    next.layout.y = Math.max(
      0,
      ...state.modules
        .filter((m) => m.id !== next.id)
        .map((m) => m.layout.y + m.layout.h),
    );
    next.layout.w = Math.min(24, next.layout.w + 2);
    next.layout.x = 0;
    changeType = "metadata";
  } else if (next.type === "document") {
    next.content += `\n\n## Mock 提案\n\n${text}`;
  } else if (next.type === "planner") {
    if (/删除|移除|remove/i.test(text)) next.tasks = next.tasks.slice(1);
    else if (/完成|done/i.test(text) && next.tasks.length)
      next.tasks[0].done = true;
    else if (/修改|改写|edit/i.test(text) && next.tasks.length)
      next.tasks[0].title = text.slice(0, 120);
    else
      next.tasks.push({
        id: uid(),
        title: text.slice(0, 120),
        done: false,
        time: "待安排",
        tag: "Mock 演示",
      });
  } else if (next.type === "dashboard") {
    next.dashboardConfig = {
      metrics: /文档|document/i.test(text)
        ? ["document_count", "module_count"]
        : ["tasks_done", "tasks_total"],
      plannerIds: /筛选|filter/i.test(text)
        ? state.modules
            .filter((m) => m.type === "planner")
            .slice(0, 1)
            .map((m) => m.id)
        : [],
      title: "当前工作区真实数据",
    };
  }
  return {
    id: uid(),
    kind: "module_changes",
    proposalId: uid(),
    title: `${before ? "更新" : "创建"}：${next.title}`,
    description:
      "Mock 界面演示提案；检查完整差异后再批准。真实 Provider 的模块操作在原生应用验收。",
    moduleId: before?.id ?? null,
    moduleType: next.type,
    filePath: next.filePath,
    before: JSON.stringify(before ?? null, null, 2),
    after: JSON.stringify(next, null, 2),
    revision: before?.moduleRevision ?? null,
    changes: { type: changeType },
  };
}
function start(text: string, moduleId: string | null) {
  if (currentPolicy().effective === "disabled")
    throw new Error("当前权限禁止 Agent 运行");
  if (state.runStatus === "running" || state.approvals.length)
    throw new Error("请先完成或取消当前任务。");
  state.sessionId ||= uid();
  state.runStatus = "running";
  state.messages.push({ id: uid(), role: "user", text, timestamp: now() });
  event("session_started", "session/prompt · " + text);
  const reply = {
    id: uid(),
    role: "assistant" as const,
    text: "",
    timestamp: now(),
  };
  state.messages.push(reply);
  const selected = state.modules.find((m) => m.id === moduleId);
  const fail = /失败|error|fail/i.test(text),
    edit =
      /修改|改写|润色|编辑|移除|删除|完成|新增|筛选|移动|缩放|尺寸|改名|标题|write|edit|remove|done|filter|move|resize|rename/i.test(
        text,
      );
  const create = /创建|计划|看板|create|plan/i.test(text);
  const output = fail
    ? "正在验证错误恢复流程。此任务将产生一次可恢复的模拟错误。"
    : edit
      ? "我会先读取文档并生成修改建议。你可以检查差异，批准后才会写入工作区。"
      : create
        ? "我会把目标整理为一个可持续使用的模块。下面是模块提议，确认后即可放到画布上。"
        : "收到。在当前原型中，你可以让我创建计划、修改文档或演示失败与取消。所有文件修改都会先生成审批。";
  let pos = 0;
  const step = () => {
    pos += /长任务|long/i.test(text) ? 1 : 5;
    reply.text = output.slice(0, pos);
    event("message_delta", output.slice(Math.max(0, pos - 5), pos));
    if (pos < output.length) {
      save();
      timer = setTimeout(step, 120);
      return;
    }
    if (fail) {
      state.runStatus = "failed";
      event("failed", "模拟任务失败，可以直接重试");
    } else if (edit && selected && selected.type !== "conversation") {
      const proposal = mockModuleProposal(text, selected);
      if (proposal) propose(proposal);
    } else if (edit) {
      const mod =
        state.modules.find((m) => m.id === moduleId && m.type === "document") ||
        state.modules.find((m) => m.type === "document");
      if (mod)
        propose({
          id: uid(),
          kind: "write_file",
          title: "更新文档：" + mod.title,
          description: "添加本周行动安排；批准后写入。",
          moduleId: mod.id,
          moduleType: null,
          filePath: mod.filePath,
          before: mod.content,
          after:
            mod.content +
            "\n\n## 本周行动安排\n\n1. 明确目标与交付标准。\n2. 为关键任务安排专注时间。\n3. 在周末回顾成果并调整计划。",
          revision: mod.revision,
        });
      else {
        state.runStatus = "failed";
        event("failed", "请先创建文档模块");
      }
    } else if (create) {
      const proposal = mockModuleProposal(text, undefined, true);
      if (proposal) propose(proposal);
    } else {
      state.runStatus = "completed";
      event("completed", "本轮对话已完成");
    }
    save();
  };
  timer = setTimeout(step, 180);
}
function propose(a: Approval) {
  a.origin ??= "agent";
  a.epoch = currentPolicy().epoch;
  if (a.proposalId)
    state.moduleProposals!.push({
      id: a.proposalId,
      status: "pending",
      title: a.title,
      moduleId: a.moduleId,
      summary: "等待审阅，尚未写入",
      createdAt: now(),
      updatedAt: now(),
    });
  if (
    a.origin === "agent" &&
    currentPolicy().effective === "full" &&
    a.changes?.type !== "metadata"
  ) {
    applyProposal(a);
    state.runStatus = "completed";
    event("agent_action_applied", `完全访问下已应用：${a.title}`);
    return;
  }
  state.approvals.push(a);
  state.runStatus = "waiting_approval";
  event("permission_requested", a.title);
}
export const preview = {
  get: async () => copy(),
  subscribe: (fn: (s: WorkspaceSnapshot) => void) => {
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  },
  dispatch: async (
    a: WorkspaceAction,
    expectedRoot: string,
    expectedGeneration: string,
  ) => {
    if (
      expectedRoot !== state.rootPath ||
      !expectedGeneration ||
      expectedGeneration !== state.workspaceGeneration
    )
      throw new Error("工作区已切换，此操作已取消；请在当前工作区重新操作。");
    const mod =
      "moduleId" in a
        ? state.modules.find((m) => m.id === a.moduleId)
        : undefined;
    switch (a.type) {
      case "create_module": {
        const m = moduleOf(a.moduleType, a.title);
        m.layout.y = Math.max(
          0,
          ...state.modules.map((m) => m.layout.y + m.layout.h),
        );
        state.modules.push(m);
        event("module_created", a.title);
        break;
      }
      case "rename_module":
        if (mod) {
          mod.title = a.title;
          mod.moduleRevision = uid();
        }
        break;
      case "close_module":
        state.modules = state.modules.filter((m) => m.id !== a.moduleId);
        break;
      case "duplicate_module":
        if (mod) {
          const m = structuredClone(mod);
          m.id = uid();
          m.title += " · 副本";
          m.layout.y = Math.max(
            0,
            ...state.modules.map((m) => m.layout.y + m.layout.h),
          );
          state.modules.push(m);
        }
        break;
      case "set_layouts":
        for (const l of a.layouts) {
          const m = state.modules.find((m) => m.id === l.id);
          if (m) {
            m.layout = l.layout;
            m.moduleRevision = uid();
          }
        }
        event("layout_updated", "画布布局已保存");
        break;
      case "toggle_task": {
        const t = mod?.tasks.find((t) => t.id === a.taskId);
        if (t && mod) {
          t.done = !t.done;
          mod.moduleRevision = uid();
        }
        break;
      }
      case "add_task":
        mod?.tasks.push({
          id: uid(),
          title: a.title,
          done: false,
          time: "待安排",
          tag: "任务",
        });
        if (mod) mod.moduleRevision = uid();
        break;
      case "edit_document":
        if (mod)
          propose({
            id: uid(),
            kind: "write_file",
            title: "保存文档修改",
            description: "检查以下修改后应用。",
            moduleId: mod.id,
            moduleType: null,
            filePath: mod.filePath,
            before: mod.content,
            after: a.content,
            revision: a.revision,
            origin: "user",
          });
        break;
      case "decide_approval": {
        const p = state.approvals.find((p) => p.id === a.approvalId);
        if (!p) throw new Error("审批已失效");
        if (
          a.allow &&
          p.origin !== "user" &&
          (currentPolicy().effective === "restricted" ||
            currentPolicy().effective === "disabled" ||
            p.epoch !== currentPolicy().epoch)
        )
          throw new Error("当前权限不允许执行此 Agent 提议，或审批已失效");
        if (a.allow) {
          applyProposal(p);
        }
        if (!a.allow)
          proposalResult(p, "rejected", "用户已拒绝，工作区内容不变");
        state.approvals = state.approvals.filter((x) => x.id !== p.id);
        state.runStatus = state.approvals.length
          ? "waiting_approval"
          : "completed";
        event(
          a.allow ? "approval_granted" : "approval_denied",
          a.allow ? "已批准并应用" : "已拒绝，原文件保持不变",
        );
        state.messages.push({
          id: uid(),
          role: "assistant",
          text: a.allow
            ? "已完成，结果已保存在工作区。"
            : "已拒绝这次修改，原文件保持不变。",
          timestamp: now(),
        });
        break;
      }
      case "set_permission_mode":
        updatePolicy(currentPolicy().system, a.mode);
        break;
      case "set_workspace_policy":
        updatePolicy(currentPolicy().system, a.mode);
        break;
      case "set_system_policy":
        updatePolicy(a.mode, currentPolicy().local);
        break;
      case "confirm_hermes_scope":
      case "confirm_agent_scope":
        if (
          currentPolicy().effective === "ask" &&
          currentPolicy().agentScopeAccepted !== a.accepted
        ) {
          currentPolicy().epoch += 1;
          clearTimeout(timer);
          invalidateAgentProposals("运行范围或 Agent 配置变化，旧提案已取消");
          if (
            state.runStatus === "running" ||
            state.runStatus === "waiting_approval"
          )
            state.runStatus = state.approvals.length
              ? "waiting_approval"
              : "cancelled";
          event(
            "agent_scope_changed",
            `${providerName(state.agent)} 运行范围确认已变化，旧 Agent 动作已失效`,
          );
        }
        currentPolicy().agentScopeAccepted = a.accepted;
        currentPolicy().scopeProvider = a.accepted
          ? agentProvider(state.agent)
          : null;
        currentPolicy().hermesScopeAccepted =
          agentProvider(state.agent) === "hermes" && a.accepted;
        break;
      case "set_overlap":
        state.allowOverlap = a.allow;
        break;
      case "save_agent":
        if (JSON.stringify(state.agent) !== JSON.stringify(a.agent)) {
          clearTimeout(timer);
          if (state.runStatus === "running") state.runStatus = "cancelled";
          invalidateAgentProposals("运行范围或 Agent 配置变化，旧提案已取消");
          currentPolicy().hermesScopeAccepted = false;
          currentPolicy().agentScopeAccepted = false;
          currentPolicy().scopeProvider = null;
          currentPolicy().epoch += 1;
        }
        state.agent = a.agent;
        state.agent.probeStatus =
          a.agent.transport === "mock" ? "ready" : "not_connected";
        state.connection = undefined;
        break;
      case "probe_agent":
        if (state.agent.transport === "stdio")
          throw new Error(
            `真实 ${providerName(state.agent)} 探测仅支持原生应用，Web 预览不会启动或模拟该 Provider。`,
          );
        event(
          "probe",
          state.agent.transport === "mock"
            ? "Mock Agent 能力协商成功"
            : "真实 Agent 需要原生应用",
        );
        break;
      case "connect_agent":
        throw new Error(
          `真实 ${providerName(state.agent)} 连接仅支持原生应用，Web 预览不会回退到 Mock。`,
        );
      case "disconnect_agent":
        state.connection = {
          status: "disconnected",
          message: `Web 预览未启动真实 ${providerName(state.agent)}。`,
          providerSessionId: null,
          capabilities: null,
          hermesVersion: null,
        };
        break;
      case "permission_reply":
        throw new Error("Web 预览中没有真实 Agent 权限请求。");
      case "prompt":
        if (state.agent.transport !== "mock")
          throw new Error(
            `真实 ${providerName(state.agent)} 任务仅支持原生应用，Web 预览不会回退到 Mock。`,
          );
        start(a.text, a.moduleId);
        break;
      case "cancel":
        clearTimeout(timer);
        state.runStatus = "cancelled";
        invalidateAgentProposals("任务已取消，未执行提案");
        event("cancelled", "任务已取消");
        break;
    }
    return save();
  },
};
