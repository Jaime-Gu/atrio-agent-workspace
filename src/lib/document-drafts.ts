import { useCallback, useSyncExternalStore } from "react";

export interface DocumentDraft {
  text: string;
  baseRevision: string | null;
  submitted: string | null;
  saving: boolean;
  hadApproval: boolean;
  error: string;
}

// Drafts survive component/page/workspace switches for this app session only.
// A JSON tuple keeps even unusual path/ID combinations in separate namespaces.
const drafts = new Map<string, DocumentDraft>();
const listeners = new Map<string, Set<() => void>>();

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
  const getSnapshot = useCallback(() => drafts.get(key), [key]);
  const draft = useSyncExternalStore(subscribe, getSnapshot);
  const update = useCallback(
    (
      change:
        | DocumentDraft
        | undefined
        | ((previous: DocumentDraft | undefined) => DocumentDraft | undefined),
    ) => {
      const previous = drafts.get(key);
      const next = typeof change === "function" ? change(previous) : change;
      if (next === previous) return;
      if (next) drafts.set(key, next);
      else drafts.delete(key);
      listeners.get(key)?.forEach((listener) => listener());
    },
    [key],
  );
  return [draft, update] as const;
}
