import { afterEach, describe, expect, it, vi } from 'vitest';
import { createLatestFrameScheduler } from './latest-frame-scheduler';

describe('latest frame scheduler', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('coalesces pointer updates and applies only the latest value per frame', () => {
    let callback: FrameRequestCallback | null = null;
    vi.stubGlobal('requestAnimationFrame', vi.fn((next: FrameRequestCallback) => {
      callback = next;
      return 7;
    }));
    vi.stubGlobal('cancelAnimationFrame', vi.fn());
    const apply = vi.fn();
    const scheduler = createLatestFrameScheduler(apply);

    scheduler.schedule({ x: 1, y: 2 });
    scheduler.schedule({ x: 8, y: 9 });
    expect(apply).not.toHaveBeenCalled();
    const runFrame = callback as unknown as FrameRequestCallback;
    runFrame(0);

    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith({ x: 8, y: 9 });
  });

  it('flushes the latest value and cancels a queued frame', () => {
    vi.stubGlobal('requestAnimationFrame', vi.fn(() => 11));
    const cancel = vi.fn();
    vi.stubGlobal('cancelAnimationFrame', cancel);
    const apply = vi.fn();
    const scheduler = createLatestFrameScheduler(apply);

    scheduler.schedule(1);
    scheduler.schedule(2);
    scheduler.flush();

    expect(cancel).toHaveBeenCalledWith(11);
    expect(apply).toHaveBeenCalledWith(2);
  });
});
