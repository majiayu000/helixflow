import { expect, test, type Page } from '@playwright/test';
import { writeFileSync } from 'node:fs';
import { CANVAS_VIDEO_DECODER_LIMIT } from '../src/components/graph-canvas-video-decoder';

type ScaleRow = {
  scenario: string;
  logicalNodes: number;
  videoNodes: number;
  mountedNodes: number;
  videoElements: number;
  readyVideos: number;
  overview: boolean;
  zoom: string | null;
  heapMB: number | null;
  domElements: number;
  idleFps: number;
  panFps: number;
  coldMs: number;
};

test.describe('large canvas scale', () => {
  test.setTimeout(180_000);

  test('measures viewport cull, overview LOD, and video decoder pressure', async ({ page }) => {
    const report: ScaleRow[] = [];

    report.push(await loadScenario(page, '4000-text-default', '/e2e/canvas.html?nodes=4000&ws=scale-text'));
    expect(report.at(-1)?.mountedNodes).toBeLessThan(100);
    expect(report.at(-1)?.overview).toBe(false);

    await page.getByRole('button', { name: '适应全部节点' }).click();
    await expect(page.locator('.flow-canvas-overview')).toBeVisible();
    report.push(await sampleScenario(page, '4000-text-fit', 0));
    expect(report.at(-1)?.overview).toBe(true);
    expect(report.at(-1)?.mountedNodes).toBeLessThan(100);

    report.push(await loadScenario(page, '200-video-packed', '/e2e/canvas.html?nodes=220&videos=200&pack=1&ws=scale-pack'));
    await expect.poll(() => page.locator('video').count()).toBe(CANVAS_VIDEO_DECODER_LIMIT);
    report[report.length - 1] = await sampleScenario(page, '200-video-packed', report.at(-1)!.coldMs);
    expect(report.at(-1)?.mountedNodes).toBeGreaterThan(150);
    expect(report.at(-1)?.videoElements).toBe(CANVAS_VIDEO_DECODER_LIMIT);

    report.push(await loadScenario(page, '4000-mixed-spread', '/e2e/canvas.html?nodes=4000&videos=2000&ws=scale-spread'));
    expect(report.at(-1)?.mountedNodes).toBeLessThan(120);
    expect(report.at(-1)?.videoElements).toBeLessThanOrEqual(CANVAS_VIDEO_DECODER_LIMIT);

    const outPath = '/tmp/helixflow-canvas-scale.json';
    writeFileSync(outPath, `${JSON.stringify(report, null, 2)}\n`);
    console.log(`CANVAS_SCALE ${JSON.stringify(report)}`);
  });
});

async function loadScenario(page: Page, scenario: string, url: string): Promise<ScaleRow> {
  const startedAt = Date.now();
  await page.goto(url);
  await expect(page.locator('.react-flow')).toBeVisible();
  await expect.poll(() => page.locator('.react-flow__node').count()).toBeGreaterThan(0);
  return sampleScenario(page, scenario, Date.now() - startedAt);
}

async function sampleScenario(page: Page, scenario: string, coldMs: number): Promise<ScaleRow> {
  const idleFps = fpsFromIntervals(await measureFrameIntervals(page, 1_500, false));
  const panFps = fpsFromIntervals(await measureFrameIntervals(page, 1_500, true));
  const snapshot = await page.evaluate(() => {
    const memory = (performance as Performance & { memory?: { usedJSHeapSize: number } }).memory;
    const videos = [...document.querySelectorAll('video')];
    return {
      logicalNodes: window.__helixflowE2E.logicalNodeCount,
      videoNodes: window.__helixflowE2E.videoNodeCount,
      mountedNodes: document.querySelectorAll('.react-flow__node').length,
      videoElements: videos.length,
      readyVideos: videos.filter((video) => video.readyState >= 2).length,
      overview: Boolean(document.querySelector('.flow-canvas-overview')),
      zoom: document.querySelector('.flow-view-zoom')?.textContent ?? null,
      heapMB: memory ? Math.round(memory.usedJSHeapSize / 1_048_576) : null,
      domElements: document.querySelectorAll('*').length,
    };
  });
  return { scenario, ...snapshot, idleFps, panFps, coldMs };
}

async function measureFrameIntervals(page: Page, durationMs: number, pan: boolean): Promise<number[]> {
  return page.evaluate(async ({ duration, pan: shouldPan }) => {
    const pane = document.querySelector<HTMLElement>('.react-flow__pane');
    if (!pane) throw new Error('React Flow pane is unavailable');
    const samples: number[] = [];
    const startedAt = performance.now();
    let previous = startedAt;
    let direction = 1;
    return new Promise<number[]>((resolve) => {
      const sample = (now: number) => {
        samples.push(now - previous);
        previous = now;
        if (shouldPan) {
          direction *= -1;
          pane.dispatchEvent(new WheelEvent('wheel', {
            bubbles: true,
            cancelable: true,
            clientX: 640,
            clientY: 360,
            deltaY: direction * 24,
          }));
        }
        if (now - startedAt >= duration) {
          resolve(samples.slice(1));
          return;
        }
        requestAnimationFrame(sample);
      };
      requestAnimationFrame(sample);
    });
  }, { duration: durationMs, pan });
}

function fpsFromIntervals(intervals: number[]): number {
  if (intervals.length === 0) return 0;
  const sorted = [...intervals].sort((left, right) => left - right);
  const p50 = sorted[Math.floor(sorted.length * 0.5)]!;
  return Math.round((1000 / Math.max(1, p50)) * 10) / 10;
}
