import { useSyncExternalStore } from 'react';
type Snapshot = {
  pendingPrompt: string | null;
};

let snapshot: Snapshot = {
  pendingPrompt: null,
};
const listeners = new Set<() => void>();

function emit(): void {
  snapshot = { ...snapshot };
  for (const listener of listeners) listener();
}

export function useCreationStore(): Snapshot {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => snapshot,
    () => snapshot,
  );
}

export function setPendingPrompt(prompt: string | null): void {
  snapshot = { ...snapshot, pendingPrompt: prompt };
  emit();
}
