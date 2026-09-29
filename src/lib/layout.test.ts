import { describe, expect, it } from "vitest";
import { clampLayout, moveModule, overlaps } from "./layout";
import type { WorkspaceModule } from "./types";
const m = (id: string, x: number, y: number): WorkspaceModule => ({
  id,
  type: "planner",
  title: id,
  status: "idle",
  layout: { x, y, w: 12, h: 30 },
  tasks: [],
  content: "",
  filePath: null,
  revision: null,
});
describe("canvas layout", () => {
  it("keeps resize inside the 24-column canvas and manifest minimums", () => {
    expect(clampLayout({ x: 25, y: -2, w: 40, h: 2 })).toEqual({
      x: 0,
      y: 0,
      w: 24,
      h: 24,
    });
  });
  it("pushes cascaded collisions but preserves the user position and unrelated modules", () => {
    const r = moveModule(
      [m("a", 0, 0), m("b", 0, 30), m("c", 0, 60), m("d", 12, 0)],
      "a",
      { x: 0, y: 30, w: 12, h: 30 },
      false,
    );
    expect(r.map((x) => x.layout.y)).toEqual([30, 60, 90, 0]);
    for (let i = 0; i < r.length; i++)
      for (let j = i + 1; j < r.length; j++)
        expect(overlaps(r[i].layout, r[j].layout)).toBe(false);
  });
  it("allows intentional overlap without moving other modules", () => {
    expect(
      moveModule(
        [m("a", 0, 0), m("b", 0, 30)],
        "a",
        { x: 0, y: 30, w: 12, h: 30 },
        true,
      )[1].layout.y,
    ).toBe(30);
  });
});
