// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

describe("workspace dispatch identity", () => {
  beforeEach(() => {
    vi.resetModules();
    invoke.mockReset();
    const storage = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      clear: () => storage.clear(),
    });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  });
  afterEach(() => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    localStorage.clear();
    vi.unstubAllGlobals();
  });

  it("passes the captured Host generation without looking up the current workspace", async () => {
    const { dispatchAction } = await import("./api");
    const action = { type: "set_overlap", allow: true } as const;
    await dispatchAction(action, "/workspace", "generation-at-click");
    expect(invoke).toHaveBeenCalledExactlyOnceWith("dispatch", {
      action,
      expectedRoot: "/workspace",
      expectedGeneration: "generation-at-click",
    });
  });

  it("fails closed when an old snapshot has no Host generation", async () => {
    const { dispatchAction } = await import("./api");
    for (const generation of [undefined, "", "  "]) {
      await expect(
        dispatchAction(
          { type: "set_overlap", allow: true },
          "/workspace",
          generation,
        ),
      ).rejects.toThrow("工作区身份尚未确认");
    }
    expect(invoke).not.toHaveBeenCalled();
  });

  it("gives restored Web previews a new runtime generation and refuses stale actions", async () => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    const first = await import("./api");
    const before = await first.getWorkspace();
    await first.dispatchAction(
      { type: "set_overlap", allow: true },
      before.rootPath,
      before.workspaceGeneration,
    );
    vi.resetModules();
    const restored = await import("./api");
    const after = await restored.getWorkspace();
    expect(after.workspaceGeneration).toBeTruthy();
    expect(after.workspaceGeneration).not.toBe(before.workspaceGeneration);
    expect(after.allowOverlap).toBe(true);
    await expect(
      restored.dispatchAction(
        { type: "set_overlap", allow: false },
        before.rootPath,
        before.workspaceGeneration,
      ),
    ).rejects.toThrow("工作区已切换");
    expect((await restored.getWorkspace()).allowOverlap).toBe(true);
  });
});
