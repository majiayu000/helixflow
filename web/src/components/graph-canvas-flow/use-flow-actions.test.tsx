import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CanvasSnapshotUpdate } from '../../types';
import { flushActions } from '../../test-utils';
import { useFlowActions } from './use-flow-actions';
import type { WorkflowFlowNode } from './types';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('useFlowActions spatial persistence', () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(async () => {
    if (renderer) await act(async () => renderer?.unmount());
    renderer = null;
  });

  it('sends moves and resizes to the canvas snapshot boundary', async () => {
    const snapshots: CanvasSnapshotUpdate[] = [];
    const proposals = vi.fn();
    let actions!: ReturnType<typeof useFlowActions>;
    function Harness() {
      actions = useFlowActions({
        versionId: 'ver_1',
        nodes: [{
          id: 'video',
          nodeType: 'video.text_to_video',
          title: 'Video',
          category: 'Video',
          status: 'queued',
          position: { x: 10, y: 20 },
          size: { width: 240, height: 160 },
          provider: null,
          summary: 'Video',
        }],
        edges: [],
        definitionByType: new Map(),
        onCreateProposal: proposals,
        onSaveCanvasSnapshot: async (update) => { snapshots.push(update); },
        onMutationRejected: vi.fn(),
        setStatus: vi.fn(),
      });
      return null;
    }
    await act(async () => { renderer = create(<Harness />); });

    actions.commitMove([
      { id: 'video', position: { x: 30, y: 40 } } as WorkflowFlowNode,
    ]);
    actions.commitResize('video', 280, 190);
    await flushActions();

    expect(snapshots).toEqual([
      { positions: [{ id: 'video', x: 30, y: 40 }] },
      { sizes: [{ id: 'video', width: 280, height: 190 }] },
    ]);
    expect(proposals).not.toHaveBeenCalled();
  });
});
