import { useMemo } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';

import { useFlowElements } from './use-flow-elements';
import type { ResizeCommit, WorkflowFlowNode } from './types';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('useFlowElements', () => {
  it('replaces node event handlers when domain data is unchanged', async () => {
    const firstCommit = vi.fn<ResizeCommit>();
    const nextCommit = vi.fn<ResizeCommit>();
    let latest: ReturnType<typeof useFlowElements> | null = null;
    let renderer: ReactTestRenderer | null = null;

    function Harness({ onResizeCommit }: { onResizeCommit: ResizeCommit }) {
      const nodes = useMemo(() => [flowNode(onResizeCommit)], [onResizeCommit]);
      latest = useFlowElements(nodes, EMPTY_EDGES);
      return null;
    }

    await act(async () => {
      renderer = create(<Harness onResizeCommit={firstCommit} />);
    });
    await act(async () => {
      renderer?.update(<Harness onResizeCommit={nextCommit} />);
    });

    result().nodes[0]!.data.onResizeCommit('node-1', 320, 180);

    expect(firstCommit).not.toHaveBeenCalled();
    expect(nextCommit).toHaveBeenCalledWith('node-1', 320, 180);
    await act(async () => renderer?.unmount());

    function result() {
      if (!latest) throw new Error('flow elements hook did not render');
      return latest;
    }
  });
});

const EMPTY_EDGES: [] = [];

function flowNode(onResizeCommit: ResizeCommit): WorkflowFlowNode {
  return {
    id: 'node-1',
    type: 'workflow',
    position: { x: 20, y: 40 },
    data: {
      node: {
        id: 'node-1',
        nodeType: 'input.text',
        title: 'Input',
        category: 'Input',
        status: 'queued',
        position: { x: 20, y: 40 },
        provider: null,
        summary: 'input.text',
      },
      artifactOutputs: [],
      diffState: null,
      dirty: false,
      locked: false,
      resizable: true,
      stepState: 'queued',
      onResizeCommit,
    },
  };
}
