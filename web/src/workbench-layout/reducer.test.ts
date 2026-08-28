import { describe, expect, it } from 'vitest';
import { createDefaultWorkbenchLayout } from './defaults';
import { reduceWorkbenchLayout, validateWorkbenchLayout } from './reducer';

describe('workbench layout reducer', () => {
  it('moves an independent container to an allowed zone', () => {
    const initial = createDefaultWorkbenchLayout();
    const result = reduceWorkbenchLayout(initial, {
      type: 'container/move',
      containerId: 'runMonitor',
      toZoneId: 'secondarySidebar',
    });

    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.state.containers.runMonitor.zoneId).toBe('secondarySidebar');
    expect(result.state.zones.panel.containerIds).toEqual(['outputs']);
    expect(result.state.zones.secondarySidebar.containerIds).toEqual([
      'conversation', 'artifactViewer', 'runMonitor',
    ]);
    expect(validateWorkbenchLayout(result.state)).toBeNull();
  });

  it('rejects moving the required canvas out of the editor', () => {
    const result = reduceWorkbenchLayout(createDefaultWorkbenchLayout(), {
      type: 'container/move',
      containerId: 'editor',
      toZoneId: 'panel',
    });

    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.error.code).toBe('forbidden_target');
    expect(result.state).toEqual(createDefaultWorkbenchLayout());
  });

  it('clamps persisted zone sizes and keeps editor visible', () => {
    const resized = reduceWorkbenchLayout(createDefaultWorkbenchLayout(), {
      type: 'zone/resize', zoneId: 'primarySidebar', sizePx: 9000,
    });
    expect(resized.ok && resized.state.zones.primarySidebar.sizePx).toBe(580);

    const hidden = reduceWorkbenchLayout(createDefaultWorkbenchLayout(), {
      type: 'zone/toggle', zoneId: 'editor',
    });
    expect(hidden.ok).toBe(false);
  });

  it('collapses panes independently without changing business placement', () => {
    const result = reduceWorkbenchLayout(createDefaultWorkbenchLayout(), {
      type: 'pane/toggleCollapsed', paneId: 'outputs',
    });
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.state.panes.outputs.collapsed).toBe(true);
    expect(result.state.panes.run.collapsed).toBe(false);
    expect(result.state.panes.outputs.containerId).toBe('outputs');
  });
});
