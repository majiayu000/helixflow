import { describe, expect, it } from 'vitest';
import { beginWorkbenchDrag, updateWorkbenchDrag, workbenchDragCommit } from './drag-controller';
import { beginWorkbenchResize, workbenchResizeSize } from './resize-controller';

describe('workbench pointer controllers', () => {
  it('requires the drag threshold and commits only a valid target', () => {
    const pending = beginWorkbenchDrag('runMonitor', 7, 100, 100);
    const below = updateWorkbenchDrag(pending, 7, 104, 103, {
      kind: 'zone', zoneId: 'secondarySidebar', index: 0,
    });
    expect(below.active).toBe(false);
    expect(workbenchDragCommit(below, 7)).toBeNull();

    const active = updateWorkbenchDrag(below, 7, 120, 100, {
      kind: 'zone', zoneId: 'secondarySidebar', index: 0,
    });
    expect(workbenchDragCommit(active, 7)).toEqual({
      containerId: 'runMonitor',
      target: { kind: 'zone', zoneId: 'secondarySidebar', index: 0 },
    });
    expect(workbenchDragCommit(active, 8)).toBeNull();
  });

  it('resizes each edge in the correct direction and clamps size', () => {
    const primary = beginWorkbenchResize('primarySidebar', 1, 100, 100, 420);
    expect(workbenchResizeSize(primary, 1, 140, 100)).toBe(460);

    const secondary = beginWorkbenchResize('secondarySidebar', 2, 900, 100, 420);
    expect(workbenchResizeSize(secondary, 2, 850, 100)).toBe(470);

    const panel = beginWorkbenchResize('panel', 3, 100, 700, 240);
    expect(workbenchResizeSize(panel, 3, 100, 630)).toBe(310);
    expect(workbenchResizeSize(panel, 4, 100, 0)).toBeNull();
  });
});
