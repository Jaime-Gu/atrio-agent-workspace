// @vitest-environment jsdom
import { act, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ModuleContent } from "./ModuleContent";
import type {
  Approval,
  WorkspaceModule,
  WorkspaceSnapshot,
} from "../lib/types";

(
  globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
let workspaceIndex = 0;
let snapshot: WorkspaceSnapshot;
let dispatch: ReturnType<typeof vi.fn<(action: unknown) => Promise<void>>>;

function documentModule(
  content = "# Original",
  revision = "revision-1",
): WorkspaceModule {
  return {
    id: "shared-module-id",
    type: "document",
    title: "Notes",
    status: "idle",
    layout: { x: 0, y: 0, w: 12, h: 43 },
    tasks: [],
    content,
    filePath: "notes/shared-module-id.md",
    revision,
  };
}

function workspace(rootPath: string): WorkspaceSnapshot {
  return {
    schemaVersion: 1,
    name: "Workspace",
    rootPath,
    modules: [documentModule()],
    messages: [],
    events: [],
    approvals: [],
    runStatus: "idle",
    sessionId: null,
    permissionMode: "ask",
    agent: {
      id: "mock",
      name: "Mock",
      transport: "mock",
      command: "",
      args: [],
      env: {},
      cwd: "",
      probeStatus: "ready",
    },
    allowOverlap: false,
    updatedAt: new Date().toISOString(),
  };
}

function approval(): Approval {
  return {
    id: "approval-1",
    kind: "write_file",
    title: "Save",
    description: "",
    moduleId: snapshot.modules[0].id,
    moduleType: "document",
    filePath: snapshot.modules[0].filePath,
    before: "# Original",
    after: "# Draft",
    revision: "revision-1",
  };
}

async function render(next = snapshot) {
  snapshot = next;
  await act(async () =>
    root.render(
      <StrictMode>
        <ModuleContent
          module={snapshot.modules[0]}
          snapshot={snapshot}
          dispatch={dispatch}
          onPrompt={() => {}}
          focused={false}
        />
      </StrictMode>,
    ),
  );
}

async function hide() {
  await act(async () => root.render(null));
}

function button(label: string): HTMLButtonElement {
  const found = [...container.querySelectorAll("button")].find(
    (element) => element.textContent?.trim() === label,
  );
  if (!found) throw new Error(`Button not found: ${label}`);
  return found;
}

async function click(label: string) {
  await act(async () => button(label).click());
}

function editor(): HTMLTextAreaElement {
  const found = container.querySelector<HTMLTextAreaElement>("textarea");
  if (!found) throw new Error("Document editor not found");
  return found;
}

async function input(
  element: HTMLTextAreaElement | HTMLInputElement,
  value: string,
) {
  await act(async () => {
    const prototype =
      element instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(
      element,
      value,
    );
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  snapshot = workspace(`/workspace-${++workspaceIndex}`);
  dispatch = vi.fn(async () => {});
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});

describe("document drafts across navigation", () => {
  it("restores the editor and draft after leaving the board, with the original revision", async () => {
    await render();
    await click("编辑");
    await input(editor(), "# Draft");
    await hide();
    await render({
      ...snapshot,
      modules: [documentModule("# External change", "revision-2")],
    });
    expect(editor().value).toBe("# Draft");
    await click("保存");
    expect(dispatch).toHaveBeenCalledWith({
      type: "edit_document",
      moduleId: "shared-module-id",
      content: "# Draft",
      revision: "revision-1",
    });
  });

  it("isolates identical module IDs in different roots and restores each workspace", async () => {
    const first = snapshot;
    const second = workspace(`${snapshot.rootPath}-second`);
    await render(first);
    await click("编辑");
    await input(editor(), "First workspace draft");
    await hide();
    await render(second);
    expect(container.querySelector("textarea")).toBeNull();
    await click("编辑");
    await input(editor(), "Second workspace draft");
    await hide();
    await render(first);
    expect(editor().value).toBe("First workspace draft");
    await hide();
    await render(second);
    expect(editor().value).toBe("Second workspace draft");
  });

  it("keeps a pending draft through unmount and restores it after rejection", async () => {
    await render();
    await click("编辑");
    await input(editor(), "# Draft");
    await click("保存");
    await render({ ...snapshot, approvals: [approval()] });
    expect(editor().disabled).toBe(true);
    await hide();
    await render();
    expect(editor().value).toBe("# Draft");
    expect(editor().disabled).toBe(true);
    await hide();
    await render({ ...snapshot, approvals: [] });
    expect(editor().value).toBe("# Draft");
    expect(editor().disabled).toBe(false);
    expect(container.textContent).toContain("写入未通过，草稿已保留");
    await click("保存");
    expect(dispatch).toHaveBeenCalledTimes(2);
  });

  it("clears an approved draft even when approval completes while away", async () => {
    await render();
    await click("编辑");
    await input(editor(), "# Draft");
    await click("保存");
    await render({ ...snapshot, approvals: [approval()] });
    await hide();
    await render({
      ...snapshot,
      approvals: [],
      modules: [documentModule("# Draft", "revision-2")],
    });
    expect(container.querySelector("textarea")).toBeNull();
    await hide();
    await render({
      ...snapshot,
      modules: [documentModule("# Later version", "revision-3")],
    });
    await click("编辑");
    expect(editor().value).toBe("# Later version");
  });

  it("recovers a rejection even if the approval was first displayed while away", async () => {
    await render();
    await click("编辑");
    await input(editor(), "# Draft");
    await click("保存");
    // The page is absent for the entire approval/rejection lifecycle.
    await hide();
    await render({ ...snapshot, approvals: [] });
    expect(editor().value).toBe("# Draft");
    expect(editor().disabled).toBe(false);
    expect(container.textContent).toContain("写入未通过，草稿已保留");
  });

  it("does not treat matching external content as approval while the proposal remains pending", async () => {
    await render();
    await click("编辑");
    await input(editor(), "# Draft");
    await click("保存");
    await render({
      ...snapshot,
      approvals: [approval()],
      modules: [documentModule("# Draft", "external-revision")],
    });
    expect(editor().value).toBe("# Draft");
    expect(editor().disabled).toBe(true);
  });

  it("preserves both text and stale revision when the Host rejects a conflict", async () => {
    dispatch.mockRejectedValue("文档已被外部修改");
    await render();
    await click("编辑");
    await input(editor(), "# Draft");
    await click("保存");
    await hide();
    await render({
      ...snapshot,
      modules: [documentModule("# External", "revision-2")],
    });
    expect(editor().value).toBe("# Draft");
    expect(container.textContent).toContain("文档已被外部修改");
    await click("保存");
    expect(dispatch).toHaveBeenLastCalledWith({
      type: "edit_document",
      moduleId: "shared-module-id",
      content: "# Draft",
      revision: "revision-1",
    });
  });

  it("updates the cached submission after its component was unmounted", async () => {
    let reject!: (reason: unknown) => void;
    dispatch.mockImplementation(
      () =>
        new Promise<void>((_, rejectRequest) => {
          reject = rejectRequest;
        }),
    );
    await render();
    await click("编辑");
    await input(editor(), "# Draft");
    await click("保存");
    await hide();
    await act(async () => reject(new Error("保存失败")));
    await render();
    expect(editor().value).toBe("# Draft");
    expect(editor().disabled).toBe(false);
    expect(container.textContent).toContain("保存失败");
  });

  it("resumes recovery after an in-flight save settles on the restored page", async () => {
    let resolve!: () => void;
    dispatch.mockImplementation(
      () =>
        new Promise<void>((resolveRequest) => {
          resolve = resolveRequest;
        }),
    );
    await render();
    await click("编辑");
    await input(editor(), "# Draft");
    await click("保存");
    await hide();
    // The Host proposal was rejected while this page was absent, but the
    // original dispatch promise is still pending when the page is restored.
    await render({ ...snapshot, approvals: [] });
    expect(editor().disabled).toBe(true);
    await act(async () => resolve());
    expect(editor().value).toBe("# Draft");
    expect(editor().disabled).toBe(false);
    expect(button("保存").disabled).toBe(false);
    expect(button("取消").disabled).toBe(false);
    expect(container.textContent).toContain("写入未通过，草稿已保留");
  });

  it("does not mistake a first submission for rejection before its approval snapshot arrives", async () => {
    let resolve!: () => void;
    dispatch.mockImplementation(
      () =>
        new Promise<void>((resolveRequest) => {
          resolve = resolveRequest;
        }),
    );
    await render();
    await click("编辑");
    await input(editor(), "# Draft");
    await click("保存");
    await act(async () => resolve());
    expect(editor().value).toBe("# Draft");
    expect(editor().disabled).toBe(true);
    expect(container.querySelector('[role="alert"]')).toBeNull();
    await render({ ...snapshot, approvals: [approval()] });
    expect(editor().disabled).toBe(true);
    await render({ ...snapshot, approvals: [] });
    expect(editor().disabled).toBe(false);
  });

  it("discards the session draft only on explicit cancel", async () => {
    await render();
    await click("编辑");
    await input(editor(), "# Draft");
    await click("取消");
    await hide();
    await render();
    expect(container.querySelector("textarea")).toBeNull();
    await click("编辑");
    expect(editor().value).toBe("# Original");
  });
});

describe("planner task title", () => {
  it("submits 120 Unicode characters, including full emoji", async () => {
    snapshot.modules = [{ ...documentModule(), type: "planner" }];
    await render();
    await click("添加任务+");
    const field = container.querySelector<HTMLInputElement>("input")!;
    await input(field, "😀".repeat(121));
    expect(field.value).toBe("😀".repeat(120));
    await act(async () =>
      container
        .querySelector("form")!
        .dispatchEvent(
          new Event("submit", { bubbles: true, cancelable: true }),
        ),
    );
    expect(dispatch).toHaveBeenCalledWith({
      type: "add_task",
      moduleId: "shared-module-id",
      title: "😀".repeat(120),
    });
  });
});

describe("compact module copy", () => {
  it("keeps the conversation welcome focused on one next action", async () => {
    snapshot.modules = [{ ...documentModule(), type: "conversation" }];
    await render();
    expect(container.textContent).toContain("说出目标，Agent 会帮你整理。");
    expect(container.textContent).not.toContain("YOUR SPACE TO MAKE THINGS");
  });

  it("uses a short planner empty state", async () => {
    snapshot.modules = [{ ...documentModule(), type: "planner", tasks: [] }];
    await render();
    expect(container.textContent).toContain("暂无任务");
    expect(container.textContent).toContain("添加任务");
  });

  it("exposes dashboard source details through an accessible label", async () => {
    snapshot.modules = [
      {
        ...documentModule(),
        type: "dashboard",
        dashboardConfig: {
          metrics: ["module_count"],
          plannerIds: [],
        },
      },
    ];
    await render();
    const source = container.querySelector<HTMLElement>(
      ".mc-dashboard-filter[role='img']",
    );
    expect(source?.textContent).toContain("数据来源");
    expect(source?.getAttribute("aria-label")).toContain("当前工作区");
  });
});

describe("Dashboard local metrics", () => {
  it("renders the selected planners' actual data and reacts to saved task changes", async () => {
    const dashboard: WorkspaceModule = {
      ...documentModule(),
      id: "dashboard",
      type: "dashboard",
      dashboardConfig: {
        metrics: ["tasks_done", "tasks_total", "document_count"],
        plannerIds: ["selected-plan"],
        title: "My selected tasks",
      },
    };
    const selected: WorkspaceModule = {
      ...documentModule(),
      id: "selected-plan",
      type: "planner",
      tasks: [{ id: "t", title: "Task", done: false, time: "", tag: "" }],
    };
    const excluded: WorkspaceModule = {
      ...selected,
      id: "other-plan",
      tasks: Array.from({ length: 5 }, (_, id) => ({
        ...selected.tasks[0],
        id: String(id),
        done: true,
      })),
    };
    snapshot.modules = [dashboard, selected, excluded, documentModule()];
    await render();
    expect(
      container.querySelector('[data-metric="tasks_done"] strong')?.textContent,
    ).toBe("0");
    expect(
      container.querySelector('[data-metric="tasks_total"] strong')
        ?.textContent,
    ).toBe("1");
    expect(
      container.querySelector('[data-metric="document_count"] strong')
        ?.textContent,
    ).toBe("1");
    expect(container.textContent).not.toContain("演示数据");
    expect(container.textContent).toContain("My selected tasks");
    selected.tasks[0].done = true;
    await render();
    expect(
      container.querySelector('[data-metric="tasks_done"] strong')?.textContent,
    ).toBe("1");
    expect(
      container
        .querySelector('[role="progressbar"]')
        ?.getAttribute("aria-valuenow"),
    ).toBe("100");
  });
});
