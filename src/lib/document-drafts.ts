import { useCallback, useSyncExternalStore } from "react";

export interface DocumentDraft {
  text: string;
  baseRevision: string | null;
  submitted: string | null;
  saving: boolean;
  hadApproval: boolean;
  error: string;
}

// Drafts survive component/page/workspace switches, refreshes, and crashes.
// A JSON tuple keeps even unusual path/ID combinations in separate namespaces.
const STORAGE_PREFIX = "atrio-document-draft:";
// localStorage is shared with the workspace preview state, so a single draft
// must not be able to crowd everything else out.
const MAX_DRAFT_TEXT = 1_000_000;

const drafts = new Map<string, DocumentDraft>();
const listeners = new Map<string, Set<() => void>>();
const persistErrors = new Map<string, string>();

export function documentDraftStorageKey(
  rootPath: string,
  moduleId: string,
): string {
  return `${STORAGE_PREFIX}${JSON.stringify([rootPath, moduleId])}`;
}

function storageKey(tupleKey: string): string {
  return `${STORAGE_PREFIX}${tupleKey}`;
}

function isValidDraft(value: unknown): value is DocumentDraft {
  if (!value || typeof value !== "object") return false;
  const draft = value as Partial<DocumentDraft>;
  return (
    typeof draft.text === "string" &&
    (draft.baseRevision === null || typeof draft.baseRevision === "string") &&
    (draft.submitted === null || typeof draft.submitted === "string") &&
    typeof draft.saving === "boolean" &&
    typeof draft.hadApproval === "boolean" &&
    typeof draft.error === "string"
  );
}

function restore(key: string): DocumentDraft | undefined {
  const cached = drafts.get(key);
  if (cached) return cached;
  try {
    const raw = localStorage.getItem(storageKey(key));
    if (!raw) return undefined;
    const parsed: unknown = JSON.parse(raw);
    if (!isValidDraft(parsed)) {
      localStorage.removeItem(storageKey(key));
      return undefined;
    }
    // A draft restored after a reload was never mid-save.
    const restored: DocumentDraft = { ...parsed, saving: false };
    drafts.set(key, restored);
    return restored;
  } catch {
    return undefined;
  }
}

function persist(key: string, next: DocumentDraft | undefined) {
  try {
    if (!next) {
      localStorage.removeItem(storageKey(key));
      persistErrors.delete(key);
      return;
    }
    if (next.text.length > MAX_DRAFT_TEXT) {
      persistErrors.set(key, "草稿过大，未能保存到本地；请先保存或缩短内容。");
      return;
    }
    localStorage.setItem(storageKey(key), JSON.stringify(next));
    persistErrors.delete(key);
  } catch {
    persistErrors.set(key, "本地存储空间不足，草稿暂未保存到本地；请先保存。");
  }
}

function notify(key: string) {
  listeners.get(key)?.forEach((listener) => listener());
}

export function useDocumentDraft(rootPath: string, moduleId: string) {
  const key = JSON.stringify([rootPath, moduleId]);
  const subscribe = useCallback(
    (listener: () => void) => {
      let group = listeners.get(key);
      if (!group) listeners.set(key, (group = new Set()));
      group.add(listener);
      return () => {
        group.delete(listener);
        if (!group.size) listeners.delete(key);
      };
    },
    [key],
  );
  const getSnapshot = useCallback(() => restore(key), [key]);
  const draft = useSyncExternalStore(subscribe, getSnapshot);
  const update = useCallback(
    (
      change:
        | DocumentDraft
        | undefined
        | ((previous: DocumentDraft | undefined) => DocumentDraft | undefined),
    ) => {
      const previous = drafts.get(key) ?? restore(key);
      const next = typeof change === "function" ? change(previous) : change;
      if (next === previous) return;
      if (next) drafts.set(key, next);
      else drafts.delete(key);
      persist(key, next);
      notify(key);
    },
    [key],
  );
  return [draft, update] as const;
}

/**
 * The latest localStorage failure for this draft, if its in-memory copy is
 * newer than what is on disk. Empty when the draft is persisted or absent.
 */
export function useDraftPersistError(
  rootPath: string,
  moduleId: string,
): string | undefined {
  const key = JSON.stringify([rootPath, moduleId]);
  const subscribe = useCallback(
    (listener: () => void) => {
      let group = listeners.get(key);
      if (!group) listeners.set(key, (group = new Set()));
      group.add(listener);
      return () => {
        group.delete(listener);
        if (!group.size) listeners.delete(key);
      };
    },
    [key],
  );
  const getSnapshot = useCallback(() => persistErrors.get(key), [key]);
  return useSyncExternalStore(subscribe, getSnapshot);
}
