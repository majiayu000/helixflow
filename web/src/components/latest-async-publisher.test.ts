import { afterEach, describe, expect, it, vi } from 'vitest';
import { createLatestAsyncPublisher } from './latest-async-publisher';

describe('latest async publisher', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('keeps one timer and publishes only the latest queued value', async () => {
    vi.useFakeTimers();
    const publish = vi.fn(async () => undefined);
    const publisher = createLatestAsyncPublisher(publish, 80, () => Date.now());

    for (let value = 0; value < 100; value += 1) publisher.schedule(value);
    expect(vi.getTimerCount()).toBe(1);

    await vi.runAllTimersAsync();
    expect(publish).toHaveBeenCalledTimes(1);
    expect(publish).toHaveBeenCalledWith(99);
  });

  it('bounds a 60Hz input stream to the configured publish rate', async () => {
    vi.useFakeTimers();
    const publish = vi.fn(async () => undefined);
    const publisher = createLatestAsyncPublisher(publish, 80, () => Date.now());

    for (let frame = 0; frame < 60; frame += 1) {
      publisher.schedule(frame);
      await vi.advanceTimersByTimeAsync(16);
    }

    expect(publish.mock.calls.length).toBeGreaterThanOrEqual(11);
    expect(publish.mock.calls.length).toBeLessThanOrEqual(13);
    expect(vi.getTimerCount()).toBeLessThanOrEqual(1);
  });

  it('coalesces updates while a publish is in flight', async () => {
    vi.useFakeTimers();
    let resolveFirst!: () => void;
    const publish = vi.fn(() => new Promise<void>((resolve) => {
      resolveFirst = resolve;
    }));
    const publisher = createLatestAsyncPublisher(publish, 80, () => Date.now());

    publisher.schedule(1);
    await vi.advanceTimersByTimeAsync(0);
    publisher.schedule(2);
    publisher.schedule(3);
    expect(publish).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);

    resolveFirst();
    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(80);
    expect(publish).toHaveBeenCalledTimes(2);
    expect(publish).toHaveBeenLastCalledWith(3);
  });

  it('flushes the latest value without waiting for the interval', async () => {
    vi.useFakeTimers();
    const publish = vi.fn(async () => undefined);
    const publisher = createLatestAsyncPublisher(publish, 80, () => Date.now());

    publisher.schedule('left');
    publisher.flush();
    await Promise.resolve();

    expect(publish).toHaveBeenCalledWith('left');
    expect(vi.getTimerCount()).toBe(0);
  });
});
