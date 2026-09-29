// @vitest-environment jsdom
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PolicyPanel, policyBlockReason } from "./PolicyPanel";
import { ReviewPanel, SettingsPanel } from "./Panels";
import type { Approval, WorkspaceSnapshot } from "../lib/types";

const runtime = vi.hoisted(() => ({ desktop: true }));
vi.mock("../lib/api", () => ({
  get isDesktop() {
    return runtime.desktop;
  },
}));
let container: HTMLDivElement, root: Root, snapshot: WorkspaceSnapshot;
const dispatch = vi.fn<(action: unknown) => Promise<void>>();
beforeEach(() => {
  vi.clearAllMocks();
  runtime.desktop = true;
  dispatch.mockResolvedValue(undefined);
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function () {
    this.open = false;
  };
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  snapshot = {
    schemaVersion: 1,
    name: "Test workspace",
    rootPath: "/test",
    modules: [],
    messages: [],
    approvals: [],
    events: [],
    runStatus: "idle",
    sessionId: null,
    permissionMode: "ask",
    policy: {
      system: "workspace",
      local: "ask",
      effective: "ask",
      source: "workspace",
      epoch: 2,
      workspaceTrusted: true,
      hermesScopeAccepted: false,
    },
    agent: {
      id: "hermes",
      name: "Hermes",
      transport: "stdio",
      command: "hermes",
      args: ["acp"],
      env: {},
      cwd: "",
      probeStatus: "ready",
    },
    allowOverlap: false,
    updatedAt: "2026-09-28T00:00:00Z",
  };
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});
async function render(node: ReactNode) {
  await act(async () => root.render(node));
}
function button(label: string) {
  return [...container.querySelectorAll("button")].find(
    (b) => b.textContent?.trim() === label,
  )!;
}
async function click(label: string) {
  const b = button(label);
  expect(b).toBeTruthy();
  await act(async () => b.click());
}
function settings() {
  return (
    <SettingsPanel
      appInfo={{ status: "loading" }}
      snapshot={snapshot}
      dispatch={dispatch}
      onClose={() => {}}
      onChoose={() => {}}
    />
  );
}
function policyPanel() {
  return (
    <PolicyPanel
      snapshot={snapshot}
      dispatch={dispatch}
      onClose={() => {}}
      onSettings={() => {}}
    />
  );
}
function approval(overrides: Partial<Approval> = {}): Approval {
  return {
    id: "p1",
    kind: "write_file",
    title: "Write",
    description: "",
    moduleId: null,
    moduleType: null,
    filePath: "notes/a.md",
    before: "a",
    after: "b",
    revision: "r1",
    origin: "agent",
    epoch: 2,
    ...overrides,
  };
}

describe("policy and Hermes interface", () => {
  it("shows the effective system policy and disables overridden local choices", async () => {
    snapshot.policy = {
      ...snapshot.policy!,
      system: "deny_all",
      effective: "disabled",
      source: "system",
      local: "full",
    };
    await render(
      <PolicyPanel
        snapshot={snapshot}
        dispatch={dispatch}
        onClose={() => {}}
        onSettings={() => {}}
      />,
    );
    expect(container.textContent).toContain("禁止运行 · 系统");
    expect(container.textContent).toContain("原工作区选择“完全访问”已保留");
    for (const button of container.querySelectorAll<HTMLButtonElement>(
      ".policy-options button",
    ))
      expect(button.disabled).toBe(true);
    expect(dispatch).not.toHaveBeenCalled();
  });
  it("requires explicit scope confirmation for Hermes ask and blocks unverified restricted mode", async () => {
    await render(settings());
    expect(button("连接 Hermes").disabled).toBe(true);
    expect(
      container.querySelector<HTMLDetailsElement>(".policy-details")?.open,
    ).toBeUndefined();
    await render(policyPanel());
    expect(container.textContent).toContain(
      "允许 Hermes 在此工作区运行；它的自有工具可能访问本机",
    );
    expect(
      container.querySelector<HTMLDetailsElement>(".policy-details")?.open,
    ).toBe(false);
    await act(async () =>
      container
        .querySelector<HTMLInputElement>(".scope-confirm input")!
        .click(),
    );
    expect(dispatch).toHaveBeenCalledWith({
      type: "confirm_agent_scope",
      accepted: true,
    });
    snapshot.policy!.hermesScopeAccepted = true;
    await render(settings());
    expect(button("连接 Hermes").disabled).toBe(false);
    await click("连接 Hermes");
    expect(dispatch).toHaveBeenLastCalledWith({ type: "connect_agent" });
    snapshot.policy!.effective = "restricted";
    await render(settings());
    expect(button("连接 Hermes").disabled).toBe(true);
    expect(policyBlockReason(snapshot)).toContain("只读模式暂不可连接");
  });
  it("requires changed Hermes configuration to be saved before accepting scope or connecting", async () => {
    snapshot.agent = {
      ...snapshot.agent,
      id: "mock",
      name: "Mock Agent",
      transport: "mock",
      command: "",
      args: [],
    };
    await render(settings());
    await act(async () => {
      const select = container.querySelector("select")!;
      select.value = "hermes";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(container.querySelector(".scope-confirm")).toBeNull();
    expect(button("连接 Hermes").disabled).toBe(true);
    await act(async () =>
      container
        .querySelector("form")!
        .dispatchEvent(
          new Event("submit", { bubbles: true, cancelable: true }),
        ),
    );
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "save_agent",
        agent: expect.objectContaining({
          id: "hermes",
          command: "hermes",
          args: ["acp"],
        }),
      }),
    );
    expect(dispatch).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "confirm_agent_scope" }),
    );
  });
  it.each(["not_installed", "not_authenticated", "stopping"] as const)(
    "shows the actual %s state without changing Agent provider",
    async (status) => {
      snapshot.connection = {
        status,
        message: "Host diagnostic",
        providerSessionId: null,
        capabilities: null,
        hermesVersion: "test-hermes",
      };
      await render(settings());
      expect(
        container.querySelector(".connection-state")?.textContent,
      ).toContain("Host diagnostic");
      expect(container.querySelector("select")?.value).toBe("hermes");
      expect(dispatch).not.toHaveBeenCalled();
      if (status === "stopping")
        expect(button("连接 Hermes").disabled).toBe(true);
    },
  );
  it("keeps Hermes connection disabled in Web even with full policy", async () => {
    runtime.desktop = false;
    snapshot.policy!.effective = "full";
    await render(settings());
    expect(button("连接 Hermes").disabled).toBe(true);
    expect(container.textContent).toContain("Web 预览仅模拟权限和 Mock");
  });
  it("allows manual document approval under deny_all, while blocking an Agent approval", async () => {
    snapshot.policy = {
      ...snapshot.policy!,
      system: "deny_all",
      source: "system",
      effective: "disabled",
    };
    snapshot.approvals = [approval({ origin: "user" })];
    await render(
      <ReviewPanel
        snapshot={snapshot}
        dispatch={dispatch}
        onClose={() => {}}
      />,
    );
    expect(button("批准并应用").disabled).toBe(false);
    snapshot.approvals = [approval()];
    await render(
      <ReviewPanel
        snapshot={snapshot}
        dispatch={dispatch}
        onClose={() => {}}
      />,
    );
    expect(button("批准并应用").disabled).toBe(true);
  });
  it("uses provider option IDs, allows reject, exposes cancel and blocks stale approvals", async () => {
    snapshot.approvals = [
      approval({
        kind: "agent_permission",
        scope: "write /test/result.md",
        options: [
          { id: "provider-allow", name: "允许这次", kind: "allow_once" },
          { id: "provider-reject", name: "拒绝这次", kind: "reject_once" },
        ],
      }),
    ];
    await render(
      <ReviewPanel
        snapshot={snapshot}
        dispatch={dispatch}
        onClose={() => {}}
      />,
    );
    await click("允许这次");
    expect(dispatch).toHaveBeenLastCalledWith({
      type: "permission_reply",
      approvalId: "p1",
      optionId: "provider-allow",
    });
    snapshot.policy!.epoch += 1;
    await render(
      <ReviewPanel
        snapshot={snapshot}
        dispatch={dispatch}
        onClose={() => {}}
      />,
    );
    expect(button("允许这次").disabled).toBe(true);
    expect(button("拒绝这次").disabled).toBe(false);
    await click("拒绝请求");
    expect(dispatch).toHaveBeenLastCalledWith({
      type: "permission_reply",
      approvalId: "p1",
      optionId: null,
    });
    await click("取消任务");
    expect(dispatch).toHaveBeenLastCalledWith({ type: "cancel" });
  });
});

describe("shared provider configuration and typed review", () => {
  it.each([
    ["claude_code", "Claude Code", "claude-agent-acp"],
    ["codex", "Codex", "codex-acp"],
  ] as const)(
    "configures %s with its adapter and no Hermes argv or scope inheritance",
    async (provider, name, command) => {
      snapshot.policy!.hermesScopeAccepted = true;
      snapshot.connection = {
        provider: "hermes",
        status: "connected",
        message: "Old Hermes connection",
        providerSessionId: "old-hermes-session",
        capabilities: {},
        hermesVersion: "legacy",
      };
      await render(settings());
      await act(async () => {
        const select = container.querySelector("select")!;
        select.value = provider;
        select.dispatchEvent(new Event("change", { bubbles: true }));
      });
      expect(button(`连接 ${name}`).disabled).toBe(true);
      expect(container.textContent).not.toContain("old-hermes-session");
      expect(container.textContent).not.toContain("Old Hermes connection");
      expect(container.querySelector(".scope-confirm")).toBeNull();
      await click("保存 Agent");
      const action = dispatch.mock.calls.at(-1)![0] as {
        agent: typeof snapshot.agent;
      };
      expect(action).toMatchObject({
        type: "save_agent",
        agent: { id: provider, provider, command, args: [] },
      });
      expect(action).not.toHaveProperty("origin");
      snapshot.agent = action.agent;
      snapshot.connection = undefined;
      await render(settings());
      expect(button(`连接 ${name}`).disabled).toBe(true);
      expect(container.querySelector(".hermes-scope")).toBeNull();
      await render(policyPanel());
      await act(async () =>
        container
          .querySelector<HTMLInputElement>(".scope-confirm input")!
          .click(),
      );
      expect(dispatch).toHaveBeenLastCalledWith({
        type: "confirm_agent_scope",
        accepted: true,
      });
      snapshot.policy!.agentScopeAccepted = true;
      snapshot.policy!.scopeProvider = provider;
      await render(settings());
      expect(button(`连接 ${name}`).disabled).toBe(false);
      snapshot.policy!.scopeProvider = "hermes";
      await render(settings());
      expect(button(`连接 ${name}`).disabled).toBe(true);
    },
  );

  it("shows task removal, metadata and pending state and sends only an approval decision", async () => {
    const before = {
      id: "planner",
      type: "planner",
      title: "Original plan",
      layout: { x: 0, y: 0, w: 12, h: 43 },
      tasks: [
        {
          id: "remove",
          title: "Review contract",
          done: false,
          time: "09:00",
          tag: "Work",
        },
      ],
      content: "",
      moduleRevision: "m1",
    };
    const after = {
      ...before,
      title: "Updated plan",
      tasks: [],
      layout: { x: 0, y: 43, w: 14, h: 43 },
    };
    snapshot.approvals = [
      approval({
        kind: "module_changes",
        moduleId: "planner",
        before: JSON.stringify(before),
        after: JSON.stringify(after),
        revision: "m1",
        proposalId: "proposal-1",
      }),
    ];
    snapshot.moduleProposals = [
      {
        id: "proposal-1",
        status: "pending",
        title: "Plan update",
        summary: "Waiting",
      },
    ];
    await render(
      <ReviewPanel
        snapshot={snapshot}
        dispatch={dispatch}
        onClose={() => {}}
      />,
    );
    expect(container.textContent).toContain("移除任务");
    expect(container.textContent).toContain("Review contract");
    expect(container.textContent).toContain("Original plan → Updated plan");
    expect(container.textContent).toContain("待审批 · 未写入");
    expect(button("取消任务")).toBeTruthy();
    await click("批准并应用");
    expect(dispatch).toHaveBeenLastCalledWith({
      type: "decide_approval",
      approvalId: "p1",
      allow: true,
    });
    snapshot.approvals = [];
    snapshot.moduleProposals![0] = {
      ...snapshot.moduleProposals![0],
      status: "conflict",
      summary: "Saved revision changed",
    };
    await render(
      <ReviewPanel
        snapshot={snapshot}
        dispatch={dispatch}
        onClose={() => {}}
      />,
    );
    expect(container.textContent).toContain("版本冲突 · 未应用");
    expect(button("批准并应用")).toBeUndefined();
  });
});

describe("bundled Codex runtime guidance", () => {
  function codexSnapshot() {
    snapshot.agent = {
      ...snapshot.agent,
      id: "codex",
      provider: "codex",
      name: "Codex",
      command: "codex-acp",
      args: [],
    };
    snapshot.connection = {
      provider: "codex",
      status: "not_installed",
      message: "ACP version check failed",
      providerSessionId: null,
      capabilities: null,
      hermesVersion: null,
      probe: {
        provider: "codex",
        hostExecutable:
          "/Applications/Atrio WorkSpace Beta.app/Contents/Resources/codex-runtime/codex",
        hostVersion: "codex-cli 0.1-test",
        executable:
          "/Applications/Atrio WorkSpace Beta.app/Contents/Resources/codex-runtime/codex-acp",
        version: "acp-test",
        hostAvailable: true,
        adapterAvailable: true,
        acpAvailable: false,
        authenticationStatus: "not_checked",
        handshakeStatus: "not_checked",
        sessionStatus: "not_checked",
        restrictedSupported: false,
        detail:
          "来源：应用内置；adapter acp-test；CLI codex-cli 0.1-test；ACP version check failed",
      },
    };
  }
  it("shows bundled source, exact CLI path and an ACP failure without claiming missing programs or authentication", async () => {
    codexSnapshot();
    await render(settings());
    expect(
      container.querySelector('[data-testid="codex-runtime-help"]')
        ?.textContent,
    ).toContain("连接时自动检查");
    expect(
      container.querySelector('[data-testid="codex-auth-help"]')?.textContent,
    ).toContain("复用本机 Codex 登录");
    const connection = container.querySelector(".connection-state")!;
    expect(connection.querySelector("strong")?.textContent).toBe(
      "ACP 检查失败",
    );
    expect(connection.querySelector("details")?.open).toBe(true);
    expect(connection.textContent).toContain(
      snapshot.connection!.probe!.detail,
    );
    expect(connection.textContent).toContain(
      snapshot.connection!.probe!.hostExecutable,
    );
    expect(connection.textContent).toContain("ACP 检查：未通过");
    expect(connection.textContent).toContain("认证/服务调用：not_checked");
    expect(connection.textContent).not.toContain("未找到");
    expect(connection.textContent).not.toContain("已认证");
    expect(dispatch).not.toHaveBeenCalled();
  });
  it("keeps account login distinct and preserves an explicitly entered external adapter path", async () => {
    codexSnapshot();
    snapshot.connection!.status = "not_authenticated";
    snapshot.connection!.message = "Authentication required";
    snapshot.connection!.probe!.acpAvailable = true;
    await render(settings());
    expect(container.querySelector(".connection-state")?.textContent).toContain(
      "请先在 Codex 完成登录",
    );
    expect(
      container.querySelector(".connection-state strong")?.textContent,
    ).toBe("待认证");
    const advanced = container.querySelector<HTMLDetailsElement>(
      '[data-testid="codex-advanced"]',
    )!;
    expect(advanced.open).toBe(false);
    await act(async () => advanced.querySelector("summary")!.click());
    expect(advanced.open).toBe(true);
    const command = [
      ...container.querySelectorAll<HTMLInputElement>("input"),
    ].find((input) => input.value === "codex-acp")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!.call(command, "/opt/custom/codex-acp");
      command.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await click("保存 Agent");
    expect(dispatch).toHaveBeenLastCalledWith(
      expect.objectContaining({
        type: "save_agent",
        agent: expect.objectContaining({
          provider: "codex",
          command: "/opt/custom/codex-acp",
          args: [],
        }),
      }),
    );
    expect(dispatch).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "connect_agent" }),
    );
  });
  it("connects an already saved and accepted Codex with one click and no separate probe", async () => {
    codexSnapshot();
    snapshot.connection = undefined;
    snapshot.agent.probeStatus = "not_connected";
    snapshot.policy!.agentScopeAccepted = true;
    snapshot.policy!.scopeProvider = "codex";
    await render(settings());
    expect(
      container.querySelector('[data-testid="codex-runtime-help"]')
        ?.textContent,
    ).toContain("连接时自动检查");
    expect(
      container.querySelector('[data-testid="codex-auth-help"]')?.textContent,
    ).toContain("复用本机 Codex 登录");
    expect(
      container.querySelector<HTMLDetailsElement>(
        '[data-testid="codex-advanced"]',
      )?.open,
    ).toBe(false);
    expect(button("连接 Codex").disabled).toBe(false);
    await click("连接 Codex");
    expect(dispatch).toHaveBeenCalledExactlyOnceWith({ type: "connect_agent" });
    expect(dispatch).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "probe_agent" }),
    );
    expect(dispatch).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "confirm_agent_scope" }),
    );
  });
});
