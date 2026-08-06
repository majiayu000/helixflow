export type LatestFrameScheduler<T> = {
  cancel: () => void;
  flush: () => void;
  schedule: (value: T) => void;
};

export function createLatestFrameScheduler<T>(apply: (value: T) => void): LatestFrameScheduler<T> {
  let frame: number | null = null;
  let latest: T | null = null;

  const flush = () => {
    if (frame !== null && typeof globalThis.cancelAnimationFrame === 'function') {
      globalThis.cancelAnimationFrame(frame);
    }
    frame = null;
    const value = latest;
    latest = null;
    if (value !== null) apply(value);
  };

  return {
    cancel: () => {
      if (frame !== null && typeof globalThis.cancelAnimationFrame === 'function') {
        globalThis.cancelAnimationFrame(frame);
      }
      frame = null;
      latest = null;
    },
    flush,
    schedule: (value) => {
      latest = value;
      if (frame !== null) return;
      if (typeof globalThis.requestAnimationFrame !== 'function') {
        flush();
        return;
      }
      frame = globalThis.requestAnimationFrame(() => {
        frame = null;
        const pending = latest;
        latest = null;
        if (pending !== null) apply(pending);
      });
    },
  };
}
