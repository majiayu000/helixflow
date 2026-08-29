import { expect, test } from '@playwright/test';

const enforceP95 = Boolean(process.env.CI || process.env.HELIXFLOW_ENFORCE_CANVAS_P95 === '1');

test.describe('4000-node canvas performance', () => {
  test.setTimeout(60_000);

  test('meets cold-interaction and viewport frame budgets', async ({ page }) => {
    const coldInteractiveMs: number[] = [];
    for (let attempt = 0; attempt < 5; attempt += 1) {
      await page.goto('about:blank');
      const startedAt = performance.now();
      await page.goto('/e2e/canvas.html?nodes=4000');
      await expect(page.locator('.react-flow__node').first()).toBeVisible();
      coldInteractiveMs.push(performance.now() - startedAt);
    }

    const frameIntervals = await page.evaluate(async () => {
      const pane = document.querySelector<HTMLElement>('.react-flow__pane');
      if (!pane) throw new Error('React Flow pane is unavailable');
      const samples: number[] = [];
      const durationMs = 10_000;
      const startedAt = performance.now();
      let previous = startedAt;
      let direction = 1;
      return new Promise<number[]>((resolve) => {
        const sample = (now: number) => {
          samples.push(now - previous);
          previous = now;
          direction *= -1;
          pane.dispatchEvent(new WheelEvent('wheel', {
            bubbles: true,
            cancelable: true,
            clientX: 720,
            clientY: 450,
            deltaY: direction * 8,
          }));
          if (now - startedAt >= durationMs) {
            resolve(samples.slice(1));
            return;
          }
          requestAnimationFrame(sample);
        };
        requestAnimationFrame(sample);
      });
    });

    const coldP95 = percentile(coldInteractiveMs, 0.95);
    const frameP95 = percentile(frameIntervals, 0.95);
    const result = {
      coldInteractiveMs: coldInteractiveMs.map(round),
      coldP95Ms: round(coldP95),
      frameP95Ms: round(frameP95),
      frameSamples: frameIntervals.length,
    };
    console.log(`CANVAS_PERF ${JSON.stringify(result)}`);

    expect(frameIntervals.length).toBeGreaterThan(300);
    if (enforceP95) {
      expect(coldP95, `cold samples: ${JSON.stringify(result.coldInteractiveMs)}`).toBeLessThanOrEqual(2_500);
      expect(frameP95, `frame samples: ${result.frameSamples}`).toBeLessThanOrEqual(32);
    }
  });
});

function percentile(values: number[], quantile: number): number {
  if (values.length === 0) throw new Error('cannot calculate a percentile without samples');
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * quantile) - 1)]!;
}

function round(value: number): number {
  return Math.round(value * 10) / 10;
}
