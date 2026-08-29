import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';

const pageErrors = new WeakMap<Page, string[]>();

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  pageErrors.set(page, errors);
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/e2e/canvas.html');
  await expect(page.locator('.react-flow')).toBeVisible();
  await expect(page.locator('.react-flow__node')).toHaveCount(2);
});

test.afterEach(async ({ page }) => {
  expect(pageErrors.get(page) ?? []).toEqual([]);
});

test('selects, moves, connects, and deletes through the real React Flow canvas', async ({ page }) => {
  const textNode = page.locator('.react-flow__node[data-id="text"]');
  const videoNode = page.locator('.react-flow__node[data-id="video"]');

  await textNode.click();
  await expect(textNode).toHaveClass(/selected/);

  const textBox = await textNode.boundingBox();
  if (!textBox) throw new Error('text node has no browser bounds');
  await page.mouse.move(textBox.x + 80, textBox.y + 24);
  await page.mouse.down();
  await page.mouse.move(textBox.x + 160, textBox.y + 84, { steps: 5 });
  await page.mouse.up();
  await expect.poll(() => proposalOps(page)).toContain('move_node');

  await page.evaluate(() => window.__helixflowE2E.proposals.splice(0));
  const source = textNode.locator('.react-flow__handle.source');
  const target = videoNode.locator('.react-flow__handle.target');
  const sourceBox = await source.boundingBox();
  const targetBox = await target.boundingBox();
  if (!sourceBox || !targetBox) throw new Error('connection handles have no browser bounds');
  await page.mouse.move(sourceBox.x + sourceBox.width / 2, sourceBox.y + sourceBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(targetBox.x + targetBox.width / 2, targetBox.y + targetBox.height / 2, { steps: 8 });
  await page.mouse.up();
  await expect.poll(() => proposalOps(page)).toContain('add_edge');

  await page.evaluate(() => window.__helixflowE2E.proposals.splice(0));
  await videoNode.click();
  await page.keyboard.press('Delete');
  await expect.poll(() => proposalOps(page)).toContain('remove_node');
});

test('resizes a selected node through React Flow NodeResizer', async ({ page }) => {
  const textNode = page.locator('.react-flow__node[data-id="text"]');
  await textNode.click();
  const resizeHandle = textNode.locator('.react-flow__resize-control.handle.bottom.right');
  await expect(resizeHandle).toBeVisible();
  const handleBox = await resizeHandle.boundingBox();
  if (!handleBox) throw new Error('resize handle has no browser bounds');

  await page.mouse.move(
    handleBox.x + handleBox.width / 2,
    handleBox.y + handleBox.height / 2,
  );
  await page.mouse.down();
  await page.mouse.move(handleBox.x + 80, handleBox.y + 60, { steps: 6 });
  await page.mouse.up();

  await expect.poll(() => proposalOps(page)).toContain('resize_node');
});

test('reverts transient drag and resize state when persistence rejects', async ({ page }) => {
  const textNode = page.locator('.react-flow__node[data-id="text"]');
  const original = await textNode.boundingBox();
  if (!original) throw new Error('text node has no initial browser bounds');

  await page.evaluate(() => window.__helixflowE2E.rejectOps.push('move_node'));
  await page.mouse.move(original.x + 80, original.y + 24);
  await page.mouse.down();
  await page.mouse.move(original.x + 170, original.y + 94, { steps: 5 });
  await page.mouse.up();

  await expect(page.locator('.canvas-status-toast')).toContainText('move_node rejected');
  await expect.poll(async () => (await textNode.boundingBox())?.x).toBeCloseTo(original.x, 0);
  await expect.poll(async () => (await textNode.boundingBox())?.y).toBeCloseTo(original.y, 0);

  await page.evaluate(() => {
    window.__helixflowE2E.rejectOps.splice(0, 1, 'resize_node');
  });
  await textNode.click();
  const resizeHandle = textNode.locator('.react-flow__resize-control.handle.bottom.right');
  const handleBox = await resizeHandle.boundingBox();
  if (!handleBox) throw new Error('resize handle has no browser bounds');
  await page.mouse.move(
    handleBox.x + handleBox.width / 2,
    handleBox.y + handleBox.height / 2,
  );
  await page.mouse.down();
  await page.mouse.move(handleBox.x + 90, handleBox.y + 70, { steps: 5 });
  await page.mouse.up();

  await expect(page.locator('.canvas-status-toast')).toContainText('resize_node rejected');
  await expect.poll(async () => (await textNode.boundingBox())?.width).toBeCloseTo(original.width, 0);
  await expect.poll(async () => (await textNode.boundingBox())?.height).toBeCloseTo(original.height, 0);
});

test('mounts only the viewport slice for a 4000-node graph', async ({ page }) => {
  await page.goto('/e2e/canvas.html?nodes=4000');
  await expect(page.locator('.flow-node-count')).toContainText('4,000 nodes');
  await expect.poll(() => page.locator('.react-flow__node').count()).toBeLessThan(100);
  await expect(page.locator('.react-flow__node[data-id="text-3999"]')).toHaveCount(0);

  await page.getByRole('button', { name: '适应全部节点' }).click();
  await expect(page.locator('.flow-canvas-overview')).toBeVisible();
  await expect.poll(() => page.locator('.react-flow__node').count()).toBeLessThan(100);
});

async function proposalOps(page: Page): Promise<string[]> {
  return page.evaluate(() => window.__helixflowE2E.proposals.flatMap(
    (proposal) => proposal.ops.map((operation) => operation.op),
  ));
}
