import type { WorkspaceAction, WorkspaceSnapshot } from "./types";
export const isDesktop = "__TAURI_INTERNALS__" in window;
export async function getWorkspace(): Promise<WorkspaceSnapshot> {
  if (!isDesktop) return (await import("./preview")).preview.get();
  return (await import("@tauri-apps/api/core")).invoke("get_workspace");
}
export async function dispatchAction(
  action: WorkspaceAction,
  expectedRoot: string,
  expectedGeneration: string | undefined,
): Promise<WorkspaceSnapshot> {
  if (!expectedGeneration?.trim())
    throw new Error("工作区身份尚未确认，请重新打开工作区后再操作。");
  if (!isDesktop)
    return (await import("./preview")).preview.dispatch(
      action,
      expectedRoot,
      expectedGeneration,
    );
  return (await import("@tauri-apps/api/core")).invoke("dispatch", {
    action,
    expectedRoot,
    expectedGeneration,
  });
}
export async function openWorkspace(path: string): Promise<WorkspaceSnapshot> {
  if (!isDesktop)
    throw new Error("网页预览使用浏览器存储。请在桌面应用中选择本地工作区。");
  return (await import("@tauri-apps/api/core")).invoke("open_workspace", {
    path,
  });
}
export async function subscribe(
  callback: (state: WorkspaceSnapshot) => void,
): Promise<() => void> {
  if (!isDesktop)
    return (await import("./preview")).preview.subscribe(callback);
  return (await import("@tauri-apps/api/event")).listen<WorkspaceSnapshot>(
    "workspace://changed",
    (e) => callback(e.payload),
  );
}
