// Rust's chars().count() counts Unicode code points, not UTF-16 code units.
export const TASK_TITLE_LIMIT = 120;

export function limitTaskTitle(value: string): string {
  return Array.from(value).slice(0, TASK_TITLE_LIMIT).join("");
}
