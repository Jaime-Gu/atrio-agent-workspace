// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  PolicyMode,
  SystemPolicy,
  WorkspaceAction,
  WorkspaceSnapshot,
} from "./types";

let preview: (typeof import("./preview"))["preview"];
let snapshot: WorkspaceSnapshot;
async function dispatch(action: WorkspaceAction) {
  snapshot = await preview.dispatch(
    action,
    snapshot.rootPath,
    snapshot.workspaceGeneration!,
  );
  return snapshot;
}
beforeEach(async () => {
  vi.resetModules();
  vi.useFakeTimers();
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  });
  preview = (await import("./preview")).preview;
  snapshot = await preview.get();
});
afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("Web policy simulation", () => {
  const systems: SystemPolicy[] = ["workspace", "allow_all", "deny_all"];
  const locals: PolicyMode[] = ["disabled", "restricted", "ask", "full"];
  it("starts with concise workspace copy and a short document", () => {
    const document = snapshot.modules.find(
      (module) => module.type === "document",
    );
    expect(document?.content).toBe("# 工作区笔记");
    expect(snapshot.messages[0]?.text).toBe("告诉我目标，我来整理成模块。");
    expect(snapshot.events[0]?.message).toBe("Web 预览已就绪");
  });
  it.each(
    systems.flatMap((system) => locals.map((local) => ({ system, local }))),
  )(
    "resolves $system × $local and restores the local preference",
    async ({ system, local }) => {
      await dispatch({ type: "set_workspace_policy", mode: local });
      await dispatch({ type: "set_system_policy", mode: system });
      expect(snapshot.policy).toMatchObject({
        system,
        local,
        effective:
          system === "allow_all"
            ? "full"
            : system === "deny_all"
              ? "disabled"
              : local,
        source: system === "workspace" ? "workspace" : "system",
      });
      await dispatch({ type: "set_system_policy", mode: "workspace" });
      expect(snapshot.policy?.effective).toBe(local);
    },
  );
  it("invalidates a running Agent on a real policy change but preserves user editing under deny_all", async () => {
    await dispatch({ type: "prompt", text: "long 创建计划", moduleId: null });
    const epoch = snapshot.policy!.epoch;
    await dispatch({ type: "set_system_policy", mode: "deny_all" });
    expect(snapshot.runStatus).toBe("cancelled");
    expect(snapshot.policy!.epoch).toBe(epoch + 1);
    await vi.runAllTimersAsync();
    expect((await preview.get()).approvals).toHaveLength(0);
    await expect(
      dispatch({ type: "prompt", text: "hello", moduleId: null }),
    ).rejects.toThrow("禁止 Agent");
    const doc = snapshot.modules.find((m) => m.type === "document")!;
    await dispatch({
      type: "edit_document",
      moduleId: doc.id,
      content: "Manual content",
      revision: doc.revision,
    });
    expect(snapshot.approvals[0].origin).toBe("user");
    await dispatch({
      type: "decide_approval",
      approvalId: snapshot.approvals[0].id,
      allow: true,
    });
    expect(snapshot.modules.find((m) => m.id === doc.id)?.content).toBe(
      "Manual content",
    );
    await dispatch({
      type: "create_module",
      moduleType: "planner",
      title: "Manual plan",
    });
    expect(snapshot.modules.some((m) => m.title === "Manual plan")).toBe(true);
  });
  it("does not cancel or increment epoch for an overridden local preference", async () => {
    await dispatch({ type: "set_system_policy", mode: "allow_all" });
    await dispatch({ type: "prompt", text: "long hello", moduleId: null });
    const epoch = snapshot.policy!.epoch;
    await dispatch({ type: "set_workspace_policy", mode: "disabled" });
    expect(snapshot.policy!.epoch).toBe(epoch);
    expect(snapshot.runStatus).toBe("running");
    expect(snapshot.policy!.local).toBe("disabled");
    expect(snapshot.policy!.effective).toBe("full");
  });
  it("keeps restricted Agent writes as proposals while allowing user edits", async () => {
    await dispatch({ type: "set_workspace_policy", mode: "restricted" });
    await dispatch({ type: "prompt", text: "修改文档", moduleId: null });
    await vi.runAllTimersAsync();
    snapshot = await preview.get();
    expect(snapshot.approvals[0].origin).toBe("agent");
    await expect(
      dispatch({
        type: "decide_approval",
        approvalId: snapshot.approvals[0].id,
        allow: true,
      }),
    ).rejects.toThrow("不允许");
    await dispatch({
      type: "decide_approval",
      approvalId: snapshot.approvals[0].id,
      allow: false,
    });
  });
  it("invalidates old Agent approvals on elevation without replaying them", async () => {
    await dispatch({ type: "prompt", text: "创建计划", moduleId: null });
    await vi.runAllTimersAsync();
    snapshot = await preview.get();
    const approvalId = snapshot.approvals[0].id;
    const before = snapshot.modules.length;
    await dispatch({ type: "set_workspace_policy", mode: "full" });
    expect(snapshot.approvals).toHaveLength(0);
    expect(snapshot.modules.length).toBe(before);
    await expect(
      dispatch({ type: "decide_approval", approvalId, allow: true }),
    ).rejects.toThrow("审批已失效");
    await dispatch({ type: "prompt", text: "创建计划", moduleId: null });
    await vi.runAllTimersAsync();
    snapshot = await preview.get();
    expect(snapshot.modules.length).toBe(before + 1);
    expect(snapshot.approvals).toHaveLength(0);
  });
  it("never claims a native Hermes connection or falls back to Mock", async () => {
    await dispatch({
      type: "save_agent",
      agent: {
        ...snapshot.agent,
        id: "hermes",
        name: "Hermes",
        transport: "stdio",
        command: "hermes",
        args: ["acp"],
      },
    });
    for (const action of [
      { type: "probe_agent" },
      { type: "connect_agent" },
      { type: "prompt", text: "hello", moduleId: null },
    ] as WorkspaceAction[]) {
      await expect(dispatch(action)).rejects.toThrow(
        "Web 预览不支持真实 Hermes",
      );
    }
    expect((await preview.get()).agent.transport).toBe("stdio");
    expect((await preview.get()).connection?.status).not.toBe("connected");
    expect((await preview.get()).runStatus).toBe("idle");
  });
  it("changes the policy epoch when ask-mode scope is accepted or withdrawn", async () => {
    const before = snapshot.policy!.epoch;
    await dispatch({ type: "confirm_hermes_scope", accepted: true });
    expect(snapshot.policy!.epoch).toBe(before + 1);
    await dispatch({ type: "confirm_hermes_scope", accepted: true });
    expect(snapshot.policy!.epoch).toBe(before + 1);
    await dispatch({ type: "confirm_hermes_scope", accepted: false });
    expect(snapshot.policy!.epoch).toBe(before + 2);
    expect(snapshot.policy!.hermesScopeAccepted).toBe(false);
  });
});

describe("Web module proposal lifecycle", () => {
  async function propose(text: string, moduleId: string | null) {
    await dispatch({ type: "prompt", text, moduleId });
    await vi.runAllTimersAsync();
    snapshot = await preview.get();
    return snapshot.approvals[0];
  }
  it("updates the same Planner through reviewed typed changes and records rejection and apply distinctly", async () => {
    await dispatch({
      type: "create_module",
      moduleType: "planner",
      title: "Trip plan",
    });
    const planner = snapshot.modules.at(-1)!;
    const originalTasks = planner.tasks.length;
    let p = await propose("新增行程：参观博物馆", planner.id);
    expect(p).toMatchObject({
      kind: "module_changes",
      origin: "agent",
      revision: planner.moduleRevision,
    });
    expect(snapshot.moduleProposals?.at(-1)?.status).toBe("pending");
    expect(snapshot.modules.at(-1)?.tasks).toHaveLength(originalTasks);
    await dispatch({ type: "decide_approval", approvalId: p.id, allow: false });
    expect(snapshot.moduleProposals?.at(-1)?.status).toBe("rejected");
    expect(snapshot.modules.at(-1)?.tasks).toHaveLength(originalTasks);
    p = await propose("新增行程：预约美术馆", planner.id);
    await dispatch({ type: "decide_approval", approvalId: p.id, allow: true });
    expect(snapshot.modules.at(-1)?.id).toBe(planner.id);
    expect(snapshot.modules.at(-1)?.tasks.at(-1)?.title).toContain(
      "预约美术馆",
    );
    expect(snapshot.moduleProposals?.at(-1)?.status).toBe("applied");
    await expect(
      dispatch({ type: "decide_approval", approvalId: p.id, allow: true }),
    ).rejects.toThrow("审批已失效");
    p = await propose("移除第一项任务", planner.id);
    const after = JSON.parse(p.after!);
    expect(after.tasks).toHaveLength(originalTasks);
    await dispatch({ type: "cancel" });
    expect(snapshot.moduleProposals?.at(-1)?.status).toBe("cancelled");
    expect(snapshot.modules.at(-1)?.tasks).toHaveLength(originalTasks + 1);
  });
  it("marks a metadata conflict and preserves the user's newer title", async () => {
    const doc = snapshot.modules.find((m) => m.type === "document")!;
    const p = await propose("改名为工具提案标题", doc.id);
    await dispatch({
      type: "rename_module",
      moduleId: doc.id,
      title: "My newer title",
    });
    await expect(
      dispatch({ type: "decide_approval", approvalId: p.id, allow: true }),
    ).rejects.toThrow("版本冲突");
    snapshot = await preview.get();
    expect(snapshot.modules.find((m) => m.id === doc.id)?.title).toBe(
      "My newer title",
    );
    expect(snapshot.moduleProposals?.at(-1)?.status).toBe("conflict");
    expect(snapshot.approvals).toHaveLength(0);
  });
  it("proposes declarative Dashboard configuration and requires layout review even in full mode", async () => {
    const dashboard = snapshot.modules.find((m) => m.type === "dashboard")!;
    const p = await propose("修改看板为文档指标", dashboard.id);
    expect(JSON.parse(p.after!).dashboardConfig.metrics).toContain(
      "document_count",
    );
    await dispatch({ type: "decide_approval", approvalId: p.id, allow: true });
    expect(
      snapshot.modules.find((m) => m.id === dashboard.id)?.dashboardConfig
        ?.metrics,
    ).toContain("document_count");
    await dispatch({ type: "set_workspace_policy", mode: "full" });
    const before = snapshot.modules.find((m) => m.id === dashboard.id)!.layout;
    await propose("移动和缩放当前模块", dashboard.id);
    expect(snapshot.approvals).toHaveLength(1);
    expect(snapshot.modules.find((m) => m.id === dashboard.id)?.layout).toEqual(
      before,
    );
  });
  it.each(["hermes", "claude_code", "codex"] as const)(
    "never simulates a real %s connection and invalidates old provider acceptance",
    async (provider) => {
      const { providerDescriptor } = await import("./providers");
      await dispatch({
        type: "save_agent",
        agent: providerDescriptor(provider),
      });
      await dispatch({ type: "confirm_agent_scope", accepted: true });
      expect(snapshot.policy?.scopeProvider).toBe(provider);
      await expect(dispatch({ type: "connect_agent" })).rejects.toThrow(
        "Web 预览不支持真实",
      );
      await expect(
        dispatch({ type: "prompt", text: "修改真实模块", moduleId: null }),
      ).rejects.toThrow("Web 预览不支持真实");
      await dispatch({
        type: "save_agent",
        agent: providerDescriptor(
          provider === "codex" ? "claude_code" : "codex",
        ),
      });
      expect(snapshot.policy?.agentScopeAccepted).toBe(false);
      expect(snapshot.policy?.scopeProvider).toBeNull();
    },
  );
});
