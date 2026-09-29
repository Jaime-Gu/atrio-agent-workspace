import type { Layout, WorkspaceModule } from "./types";
export const COLS = 24;
export const ROW = 8;
export const GAP = 12;
export function clampLayout(l: Layout): Layout {
  const w = Math.max(6, Math.min(24, Math.round(l.w)));
  return {
    x: Math.max(0, Math.min(24 - w, Math.round(l.x))),
    y: Math.max(0, Math.min(10000, Math.round(l.y))),
    w,
    h: Math.max(24, Math.min(120, Math.round(l.h))),
  };
}
export function overlaps(a: Layout, b: Layout): boolean {
  return (
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
  );
}
export function moveModule(
  modules: WorkspaceModule[],
  id: string,
  layout: Layout,
  allowOverlap: boolean,
) {
  const result = modules.map((m) => ({
    id: m.id,
    layout: m.id === id ? clampLayout(layout) : { ...m.layout },
  }));
  if (allowOverlap) return result;
  const moved = result.find((m) => m.id === id);
  if (!moved) return result;
  // Keep the user's chosen position. Push colliding modules down without compacting the board.
  const placed = [moved];
  for (const item of result
    .filter((m) => m !== moved)
    .sort((a, b) => a.layout.y - b.layout.y)) {
    let collisions = placed.filter((p) => overlaps(item.layout, p.layout));
    while (collisions.length) {
      item.layout.y = Math.max(
        ...collisions.map((p) => p.layout.y + p.layout.h),
      );
      collisions = placed.filter((p) => overlaps(item.layout, p.layout));
    }
    placed.push(item);
  }
  return result;
}
export function arrangeModules(modules: WorkspaceModule[]) {
  let y = 0;
  return modules.map((m, i) => {
    const first = i % 2 === 0;
    if (first && i > 0) y += 43;
    return { id: m.id, layout: { x: first ? 0 : 12, y, w: 12, h: 43 } };
  });
}
