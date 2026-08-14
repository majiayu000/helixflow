export type LatestAsyncPublisher<T> = {
  cancel: () => void;
  flush: () => void;
  schedule: (value: T) => void;
};

type TimerHandle = ReturnType<typeof setTimeout>;

export function createLatestAsyncPublisher<T>(
  publish: (value: T) => void | Promise<void>,
  intervalMs: number,
  now: () => number = Date.now,
): LatestAsyncPublisher<T> {
  let latest: T | undefined;
  let timer: TimerHandle | null = null;
  let inFlight = false;
  let lastStartedAt = Number.NEGATIVE_INFINITY;

  const clearTimer = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };

  const start = async () => {
    if (inFlight || latest === undefined) return;
    const value = latest;
    latest = undefined;
    inFlight = true;
    lastStartedAt = now();
    try {
      await publish(value);
    } catch {
      // The store owns visible connection failures. Keep the publisher usable.
    } finally {
      inFlight = false;
      queue();
    }
  };

  const queue = () => {
    if (timer !== null || inFlight || latest === undefined) return;
    const delay = Math.max(0, intervalMs - (now() - lastStartedAt));
    timer = setTimeout(() => {
      timer = null;
      void start();
    }, delay);
  };

  return {
    cancel: () => {
      clearTimer();
      latest = undefined;
    },
    flush: () => {
      clearTimer();
      void start();
    },
    schedule: (value) => {
      latest = value;
      queue();
    },
  };
}
