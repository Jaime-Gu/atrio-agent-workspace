import { describe, expect, it } from "vitest";
import { limitTaskTitle, TASK_TITLE_LIMIT } from "./task-title";

describe("task title limit", () => {
  it("limits Chinese and ASCII titles to the Host's 120 characters", () => {
    expect(limitTaskTitle("计".repeat(121))).toBe("计".repeat(120));
    expect(limitTaskTitle("x".repeat(121))).toHaveLength(TASK_TITLE_LIMIT);
  });

  it("counts astral characters as one character without splitting them", () => {
    const value = "😀".repeat(119) + "计划";
    const limited = limitTaskTitle(value);
    expect(limited).toBe("😀".repeat(119) + "计");
    expect(Array.from(limited)).toHaveLength(120);
  });

  it("matches Rust chars for combining sequences rather than graphemes", () => {
    expect(limitTaskTitle("a".repeat(118) + "e\u0301x")).toBe(
      "a".repeat(118) + "e\u0301",
    );
  });
});
