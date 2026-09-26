import { useEffect, useSyncExternalStore } from 'react';

/** Visible video cards keep a shell; only this many mount `<video src>`. */
export const CANVAS_VIDEO_DECODER_LIMIT = 16;

export const CANVAS_VIDEO_DECODER_PRIORITY = {
  visible: 1,
  selected: 2,
  hover: 3,
} as const;

type DecoderRequest = {
  order: number;
  priority: number;
};

const requests = new Map<string, DecoderRequest>();
const listeners = new Set<() => void>();
let active = new Set<string>();
let nextOrder = 0;
let scheduled = false;

export function resetCanvasVideoDecoders(): void {
  requests.clear();
  active = new Set();
  nextOrder = 0;
  scheduled = false;
  emit();
}

export function registerCanvasVideoDecoder(nodeId: string, priority: number): void {
  const current = requests.get(nodeId);
  if (current?.priority === priority) return;
  requests.set(nodeId, {
    order: current?.order ?? nextOrder++,
    priority,
  });
  scheduleRecompute();
}

export function unregisterCanvasVideoDecoder(nodeId: string): void {
  if (!requests.delete(nodeId)) return;
  scheduleRecompute();
}

export function isCanvasVideoDecoderActive(nodeId: string): boolean {
  return active.has(nodeId);
}

export function subscribeCanvasVideoDecoders(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function flushCanvasVideoDecoders(): void {
  scheduled = false;
  if (recompute()) emit();
}

export function useCanvasVideoDecoder(nodeId: string, priority: number, enabled = true): boolean {
  useEffect(() => {
    if (!enabled) return;
    registerCanvasVideoDecoder(nodeId, priority);
    return () => unregisterCanvasVideoDecoder(nodeId);
  }, [enabled, nodeId, priority]);

  return useSyncExternalStore(
    subscribeCanvasVideoDecoders,
    () => isCanvasVideoDecoderActive(nodeId),
    () => false,
  );
}

function scheduleRecompute(): void {
  if (scheduled) return;
  scheduled = true;
  queueMicrotask(() => {
    if (!scheduled) return;
    scheduled = false;
    if (recompute()) emit();
  });
}

function recompute(): boolean {
  const ranked = [...requests.entries()].sort((left, right) => {
    const priorityDelta = right[1].priority - left[1].priority;
    if (priorityDelta !== 0) return priorityDelta;
    return left[1].order - right[1].order;
  });
  const next = new Set(ranked.slice(0, CANVAS_VIDEO_DECODER_LIMIT).map(([nodeId]) => nodeId));
  if (next.size === active.size && [...next].every((nodeId) => active.has(nodeId))) {
    return false;
  }
  active = next;
  return true;
}

function emit(): void {
  for (const listener of listeners) listener();
}
