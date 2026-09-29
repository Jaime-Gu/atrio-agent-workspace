// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceAction, WorkspaceSnapshot } from "./lib/types";
import { version } from "../package.json";
import App from "./App";

const api = vi.hoisted(() => ({
  getWorkspace: vi.fn(),
  dispatchAction: vi.fn(),
  openWorkspace: vi.fn(),
  subscribe: vi.fn(),
  picker: vi.fn(),
  invoke: vi.fn(),
  desktop: true,
}));
vi.mock("./lib/api", () => ({
  ...api,
  get isDesktop() {
    return api.desktop;
  },
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: api.picker }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: api.invoke }));
vi.mock("./components/Canvas", () => ({
  Canvas: ({
    snapshot,
    contentDispatch,
    onInspect,
  }: {
    snapshot: WorkspaceSnapshot;
    contentDispatch: (action: WorkspaceAction) => Promise<void>;
    onInspect: (id: string) => void;
  }) => (
    <div>
      <output data-testid="workspace">{snapshot.rootPath}</output>
      <output data-testid="generation">{snapshot.workspaceGeneration}</output>
      <output data-testid="run-status">{snapshot.runStatus}</output>
      <output data-testid="sequence">{snapshot.events.at(-1)?.seq}</output>
      <button
        data-testid="action"
        onClick={() => {
          void contentDispatch({ type: "set_overlap", allow: true }).catch(
            () => {},
          );
        }}
      >
        Change layout policy
      </button>
      <button
        data-testid="inspect"
        onClick={() => onInspect(snapshot.modules[0].id)}
      >
        Inspect module
      </button>
    </div>
  ),
}));

function snapshot(
  rootPath: string,
  millis: number,
  seq: number,
  workspaceGeneration = `${rootPath}-generation`,
): WorkspaceSnapshot {
  const updatedAt = new Date(millis).toISOString();
  return {
    schemaVersion: 1,
    name: rootPath,
    rootPath,
    workspaceGeneration,
    modules: [
      {
        id: `${rootPath}-module`,
        type: "planner",
        title: "Plan",
        status: "idle",
        layout: { x: 0, y: 0, w: 12, h: 43 },
        tasks: [],
        content: "",
        filePath: null,
        revision: null,
      },
    ],
    messages: [],
    approvals: [],
    runStatus: "idle",
    sessionId: null,
    events: [
      {
        seq,
        kind: "workspace/opened",
        message: "opened",
        timestamp: updatedAt,
        sessionId: null,
      },
    ],
    permissionMode: "ask",
    allowOverlap: false,
    updatedAt,
    agent: {
      id: "mock",
      name: "Mock",
      transport: "mock",
      command: "",
      args: [],
      env: {},
      cwd: ".",
      probeStatus: "ready",
    },
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe("App identity and workspace activation", () => {
  let root: Root;
  let container: HTMLDivElement;
  let emit: (value: WorkspaceSnapshot) => void;
  beforeEach(() => {
    vi.clearAllMocks();
    api.desktop = true;
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
    api.subscribe.mockImplementation(async (callback) => {
      emit = callback;
      return () => {};
    });
    api.getWorkspace.mockResolvedValue(snapshot("/old", 2000, 10));
    api.picker.mockResolvedValue("/new");
    api.invoke.mockResolvedValue({
      version: "9.8.7",
      channel: "beta",
      appName: "Atrio WorkSpace Beta",
      candidateId: null,
      buildId: "test-native",
      sourceFingerprint: null,
      identifier: "dev.pixel.workspace",
      executablePath:
        "/Applications/Atrio WorkSpace Beta.app/Contents/MacOS/pixel-workspace",
      dataDir: "/Users/test/Library/Application Support/dev.pixel.workspace",
    });
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
  });
  async function mount() {
    await act(async () => root.render(<App />));
  }
  async function click(selector: string) {
    const button = container.querySelector<HTMLButtonElement>(selector);
    expect(button).not.toBeNull();
    await act(async () => button!.click());
  }
  function shown(name: string) {
    return container.querySelector(`[data-testid="${name}"]`)?.textContent;
  }

  it("displays effective policy and source in a persistent shield and sends system changes with the captured workspace identity", async () => {
    const initial = snapshot("/old", 2000, 10);
    initial.policy = {
      system: "deny_all",
      local: "full",
      effective: "disabled",
      source: "system",
      epoch: 3,
      workspaceTrusted: true,
      hermesScopeAccepted: false,
    };
    api.getWorkspace.mockResolvedValue(initial);
    api.dispatchAction.mockResolvedValue({
      ...initial,
      updatedAt: new Date(3000).toISOString(),
      policy: {
        ...initial.policy,
        system: "workspace",
        effective: "full",
        source: "workspace",
        epoch: 4,
      },
    });
    await mount();
    expect(
      container.querySelector(".workspace-policy-button")?.textContent,
    ).toContain("禁止运行");
    expect(
      container.querySelector(".workspace-policy-button")?.textContent,
    ).toContain("权限：禁止运行 · 系统");
    expect(
      container.querySelector(".composer-policy-note")?.textContent,
    ).toContain("仍可手动编辑");
    expect(
      container.querySelector(".composer-status")?.textContent,
    ).not.toContain("权限：");
    await click(".workspace-policy-button");
    expect(
      container.querySelector<HTMLButtonElement>(".policy-options button")
        ?.disabled,
    ).toBe(true);
    const settingsButton = [...container.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("设置与连接"),
    )!;
    await act(async () => settingsButton.click());
    const workspaceButton = [
      ...container.querySelectorAll(".system-policy button"),
    ].find((b) => b.textContent === "按工作区设置") as HTMLButtonElement;
    await act(async () => workspaceButton.click());
    expect(api.dispatchAction).toHaveBeenLastCalledWith(
      { type: "set_system_policy", mode: "workspace" },
      "/old",
      "/old-generation",
    );
    expect(
      container.querySelector(".workspace-policy-button")?.textContent,
    ).toContain("完全访问");
    expect(
      container.querySelector(".workspace-policy-button")?.textContent,
    ).toContain("权限：完全访问 · 工作区");
  });

  it("keeps the cancel control available while Hermes awaits a permission reply", async () => {
    const initial = snapshot("/old", 2000, 10);
    initial.agent = {
      ...initial.agent,
      id: "hermes",
      name: "Hermes",
      transport: "stdio",
      command: "hermes",
      args: ["acp"],
    };
    initial.runStatus = "waiting_approval";
    initial.policy = {
      system: "workspace",
      local: "ask",
      effective: "ask",
      source: "workspace",
      epoch: 3,
      workspaceTrusted: true,
      hermesScopeAccepted: true,
    };
    initial.connection = {
      status: "connected",
      message: "Ready",
      providerSessionId: "provider-1",
      capabilities: {},
      hermesVersion: "test",
    };
    initial.approvals = [
      {
        id: "native-request",
        kind: "agent_permission",
        title: "Permission",
        description: "Read file",
        moduleId: null,
        moduleType: null,
        filePath: null,
        before: null,
        after: null,
        revision: null,
        origin: "agent",
        epoch: 3,
        options: [],
      },
    ];
    api.getWorkspace.mockResolvedValue(initial);
    await mount();
    expect(
      container.querySelector('[aria-label="取消当前任务"]'),
    ).not.toBeNull();
    expect(container.querySelector(".agent-status")?.textContent).toContain(
      "Hermes 已连接",
    );
    expect(container.querySelector("dialog")?.textContent).toContain(
      "取消任务",
    );
  });

  it("shows the Web build identity consistently in the real application components", async () => {
    api.desktop = false;
    await mount();
    const label = `${version}-web`;
    expect(container.querySelector(".version")?.textContent).toBe(label);
    expect(container.querySelector(".settings-link span")?.textContent).toBe(
      label,
    );
    expect(document.title).toBe(`Atrio WorkSpace Web · ${label}`);
    await click('[aria-label="打开设置"]');
    expect(container.querySelector(".app-identity")?.textContent).toContain(
      label,
    );
    expect(container.querySelector(".app-identity")?.textContent).toContain(
      "当前 Web 构建",
    );
    expect(container.querySelector(".app-identity")?.textContent).toContain(
      window.location.origin,
    );
    expect(api.invoke).not.toHaveBeenCalled();
  });

  it("shows the selected saved-content scope and sends no module bodies or user-origin claim", async () => {
    const initial = snapshot("/old", 2000, 10);
    initial.modules[0] = {
      ...initial.modules[0],
      type: "document",
      content: "Saved private body",
      filePath: "notes/test.md",
      revision: "doc-r1",
    };
    api.getWorkspace.mockResolvedValue(initial);
    api.dispatchAction.mockResolvedValue(initial);
    await mount();
    expect(container.querySelector(".brand")?.textContent).toContain("Atrio");
    expect(shown("context-scope")).toContain("上下文：Plan（已保存）");
    expect(shown("context-scope")).toContain("工具：当前工作区");
    expect(
      container.querySelector<HTMLDetailsElement>(
        '[data-testid="context-scope"]',
      )?.open,
    ).toBe(false);
    await act(async () =>
      container
        .querySelector<HTMLDetailsElement>('[data-testid="context-scope"]')
        ?.querySelector("summary")
        ?.click(),
    );
    expect(shown("context-scope")).toContain("未保存草稿不进入本轮");
    const input = container.querySelector<HTMLTextAreaElement>(
      '[aria-label="发送给 Agent 的消息"]',
    )!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLTextAreaElement.prototype,
        "value",
      )!.set!.call(input, "Please inspect this module");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () =>
      input.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      ),
    );
    expect(api.dispatchAction).toHaveBeenLastCalledWith(
      {
        type: "prompt",
        text: "Please inspect this module",
        moduleId: "/old-module",
      },
      "/old",
      "/old-generation",
    );
  });

  it("uses the actual native identity across the header, sidebar, settings and page title", async () => {
    await mount();
    expect(container.querySelector(".version")?.textContent).toBe("9.8.7-beta");
    expect(container.querySelector(".settings-link span")?.textContent).toBe(
      "9.8.7-beta",
    );
    expect(document.title).toBe("Atrio WorkSpace Beta · 9.8.7-beta");
    await click('[aria-label="打开设置"]');
    const identity = container.querySelector(".app-identity")!;
    expect(identity.textContent).toContain("9.8.7-beta");
    expect(identity.textContent).toContain("正在运行的本地应用");
    expect(identity.textContent).toContain(
      "/Applications/Atrio WorkSpace Beta.app/Contents/MacOS/pixel-workspace",
    );
    expect(identity.textContent).toContain(
      "/Users/test/Library/Application Support/dev.pixel.workspace",
    );
    expect(container.textContent).not.toMatch(/v0\.1\.0|v0\.1\.1|P0/);
    await click('[aria-label="关闭弹窗"]');
    await click('[aria-label="打开设置"]');
    expect(api.invoke).toHaveBeenCalledExactlyOnceWith("get_app_info");
  });

  it("does not display an injected build version while native identity is pending", async () => {
    const pending = deferred<unknown>();
    api.invoke.mockReturnValue(pending.promise);
    await mount();
    expect(container.querySelector(".version")?.textContent).toBe(
      "版本核实中…",
    );
    expect(document.title).toBe("Atrio WorkSpace · 版本核实中…");
    await click('[aria-label="打开设置"]');
    expect(container.querySelector(".app-identity")?.textContent).toContain(
      "正在核实运行应用",
    );
  });

  it("makes native identity failures explicit instead of falling back to an unverified build version", async () => {
    api.invoke.mockRejectedValue(new Error("get_app_info unavailable"));
    await mount();
    expect(container.querySelector(".version")?.textContent).toBe("版本未确认");
    expect(
      container.querySelector(".version")?.getAttribute("data-version-status"),
    ).toBe("error");
    expect(container.querySelector(".settings-link span")?.textContent).toBe(
      "版本未确认",
    );
    expect(document.title).toBe("Atrio WorkSpace · 版本未确认");
    await click('[aria-label="打开设置"]');
    expect(
      container.querySelector(".app-identity [role=alert]")?.textContent,
    ).toContain("get_app_info unavailable");
    expect(container.querySelector(".app-identity .metadata")).toBeNull();
  });

  it("activates an older-dated new root, blocks actions during switching and ignores old-root events", async () => {
    const opening = deferred<WorkspaceSnapshot>();
    api.openWorkspace.mockReturnValue(opening.promise);
    await mount();
    await click(".workspace-switch");
    await click(".workspace-switch");
    expect(api.picker).toHaveBeenCalledTimes(1);
    await act(async () => {
      emit(snapshot("/new", 1000, 1));
      emit(snapshot("/old", 3000, 11));
    });
    expect(shown("workspace")).toBe("/old");
    await click('[data-testid="action"]');
    expect(api.dispatchAction).not.toHaveBeenCalled();
    await act(async () => opening.resolve(snapshot("/new", 1000, 1)));
    expect(shown("workspace")).toBe("/new");
    expect(container.querySelector('[aria-busy="true"]')).toBeNull();
    await act(async () => emit(snapshot("/old", 9000, 12)));
    expect(shown("workspace")).toBe("/new");
    api.dispatchAction.mockResolvedValue(snapshot("/new", 1100, 2));
    await click('[data-testid="action"]');
    expect(api.dispatchAction).toHaveBeenCalledWith(
      { type: "set_overlap", allow: true },
      "/new",
      "/new-generation",
    );
  });

  it("ignores a pending action response from the previous workspace", async () => {
    const action = deferred<WorkspaceSnapshot>();
    api.dispatchAction.mockReturnValue(action.promise);
    api.openWorkspace.mockResolvedValue(snapshot("/new", 4000, 1));
    await mount();
    await click('[data-testid="action"]');
    expect(api.dispatchAction).toHaveBeenCalledWith(
      { type: "set_overlap", allow: true },
      "/old",
      "/old-generation",
    );
    await click(".workspace-switch");
    await act(async () => action.resolve(snapshot("/old", 10000, 20)));
    expect(shown("workspace")).toBe("/new");
  });

  it("refreshes the existing Host state when activation fails after cancellation", async () => {
    const running = {
      ...snapshot("/old", 2000, 10),
      runStatus: "running" as const,
    };
    const cancelled = {
      ...snapshot("/old", 3000, 12),
      runStatus: "cancelled" as const,
    };
    api.getWorkspace
      .mockResolvedValueOnce(running)
      .mockResolvedValue(cancelled);
    api.openWorkspace.mockRejectedValue(
      new Error("selection could not be saved"),
    );
    await mount();
    await click(".workspace-switch");
    expect(shown("workspace")).toBe("/old");
    expect(shown("run-status")).toBe("cancelled");
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      "selection could not be saved",
    );
    expect(container.querySelector('[aria-busy="true"]')).toBeNull();
  });

  it("stops a pending multi-step settings form before it can update a copied workspace", async () => {
    const old = snapshot("/old", 2000, 10);
    const copied = {
      ...snapshot("/copy", 1000, 1),
      modules: structuredClone(old.modules),
    };
    const rename = deferred<WorkspaceSnapshot>();
    api.getWorkspace.mockResolvedValue(old);
    api.dispatchAction.mockReturnValue(rename.promise);
    api.openWorkspace.mockResolvedValue(copied);
    await mount();
    await click('[data-testid="inspect"]');
    await act(async () => {
      container
        .querySelector("dialog form")!
        .dispatchEvent(
          new Event("submit", { bubbles: true, cancelable: true }),
        );
    });
    expect(api.dispatchAction.mock.calls[0][0].type).toBe("rename_module");
    await click('[aria-label="关闭弹窗"]');
    await click(".workspace-switch");
    await act(async () =>
      rename.resolve({ ...old, updatedAt: new Date(9000).toISOString() }),
    );
    expect(shown("workspace")).toBe("/copy");
    expect(api.dispatchAction).toHaveBeenCalledTimes(1);
  });

  it("uses the workspace event sequence when same-root delivery is out of order or the clock moves back", async () => {
    await mount();
    await act(async () => emit(snapshot("/old", 1000, 11)));
    expect(shown("sequence")).toBe("11");
    await act(async () => emit(snapshot("/old", 9000, 9)));
    expect(shown("sequence")).toBe("11");
  });

  it("rejects an old generation's higher-sequence event and action response after A → B → A", async () => {
    const staleAction = deferred<WorkspaceSnapshot>();
    api.dispatchAction.mockReturnValueOnce(staleAction.promise);
    api.openWorkspace
      .mockResolvedValueOnce(snapshot("/new", 3000, 1, "B-first"))
      .mockResolvedValueOnce(snapshot("/old", 1000, 2, "A-second"));
    api.picker.mockResolvedValueOnce("/new").mockResolvedValueOnce("/old");
    await mount();
    await click('[data-testid="action"]');
    await click(".workspace-switch");
    await click(".workspace-switch");
    expect(shown("workspace")).toBe("/old");
    expect(shown("generation")).toBe("A-second");
    expect(shown("sequence")).toBe("2");
    await act(async () => {
      emit(snapshot("/old", 99000, 1000));
      emit({
        ...snapshot("/old", 99999, 2000),
        workspaceGeneration: undefined,
      });
      staleAction.resolve(snapshot("/old", 100000, 3000));
    });
    expect(shown("generation")).toBe("A-second");
    expect(shown("sequence")).toBe("2");
    await act(async () => emit(snapshot("/old", 2000, 3, "A-second")));
    expect(shown("sequence")).toBe("3");
    api.dispatchAction.mockResolvedValue(snapshot("/old", 3000, 4, "A-second"));
    await click('[data-testid="action"]');
    expect(api.dispatchAction).toHaveBeenLastCalledWith(
      { type: "set_overlap", allow: true },
      "/old",
      "A-second",
    );
  });

  it("activates an explicitly reopened same root with a fresh Host generation", async () => {
    api.picker.mockResolvedValue("/old");
    api.openWorkspace.mockResolvedValue(
      snapshot("/old", 1000, 1, "A-reopened"),
    );
    await mount();
    await click(".workspace-switch");
    expect(shown("generation")).toBe("A-reopened");
    expect(shown("sequence")).toBe("1");
    await act(async () => emit(snapshot("/old", 99000, 1000)));
    expect(shown("generation")).toBe("A-reopened");
    expect(shown("sequence")).toBe("1");
  });
});
