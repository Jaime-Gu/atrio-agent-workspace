// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ProposalDiff, parseModule } from "./ProposalReview";
import type { Approval } from "../lib/types";

(
  globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

function moduleJson(overrides: Record<string, unknown> = {}) {
  return JSON.stringify({
    id: "planner",
    type: "planner",
    title: "Plan",
    layout: { x: 0, y: 0, w: 12, h: 43 },
    tasks: [{ id: "t1", title: "Task one", done: false }],
    content: "",
    ...overrides,
  });
}

function approval(overrides: Partial<Approval> = {}): Approval {
  return {
    id: "a1",
    kind: "module_changes",
    title: "Update",
    description: "",
    moduleId: "planner",
    moduleType: "planner",
    filePath: null,
    before: moduleJson(),
    after: moduleJson({ title: "Updated plan" }),
    revision: "m1",
    origin: "agent",
    ...overrides,
  };
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});

async function renderDiff(next: Approval) {
  await act(async () => root.render(<ProposalDiff approval={next} />));
}

describe("parseModule", () => {
  it("accepts a complete module payload", () => {
    const parsed = parseModule(moduleJson());
    expect(parsed?.title).toBe("Plan");
    expect(parsed?.tasks).toHaveLength(1);
  });

  it("rejects a payload with only a type and no rendered fields", () => {
    expect(parseModule('{"type":"document"}')).toBeNull();
  });

  it("rejects invalid JSON and null", () => {
    expect(parseModule("{not json")).toBeNull();
    expect(parseModule(null)).toBeNull();
  });

  it("rejects tasks that are not an array", () => {
    expect(parseModule(moduleJson({ tasks: "not-an-array" }))).toBeNull();
  });

  it("rejects tasks without an id, title, or done flag", () => {
    expect(
      parseModule(moduleJson({ tasks: [{ title: "No id", done: false }] })),
    ).toBeNull();
  });

  it("rejects a layout with non-finite or non-numeric values", () => {
    expect(
      parseModule(moduleJson({ layout: { x: "0", y: 0, w: 12, h: 43 } })),
    ).toBeNull();
    expect(
      parseModule(moduleJson({ layout: { x: 0, y: 0, w: 12 } })),
    ).toBeNull();
  });

  it("rejects a non-string content field", () => {
    expect(parseModule(moduleJson({ content: 42 }))).toBeNull();
  });
});

describe("ProposalDiff with malformed payloads", () => {
  it("falls back to the raw text instead of crashing on a malformed proposal", async () => {
    const raw = '{"type":"document"}';
    await renderDiff(approval({ after: raw }));
    const pres = [...container.querySelectorAll("pre")];
    expect(pres).toHaveLength(2);
    expect(pres[0].textContent).toContain('"title":"Plan"');
    expect(pres[1].textContent).toContain(raw);
    expect(container.textContent).toContain("原内容");
    expect(container.textContent).toContain("提议内容");
  });

  it("treats a malformed previous version as a new module", async () => {
    await renderDiff(approval({ before: '{"type":"document"}' }));
    expect(container.textContent).toContain("创建模块");
    expect(container.textContent).toContain("Updated plan");
  });

  it("renders the diff for a well-formed proposal", async () => {
    await renderDiff(approval());
    expect(container.textContent).toContain("Plan → Updated plan");
  });
});
