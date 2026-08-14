import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useWorkbenchStore } from '../store';
import type { GraphNodeState, WorkbenchState } from '../types';
import { flushActions } from '../test-utils';

const workflowNodeRender = vi.hoisted(() => vi.fn());

vi.mock('./graph-canvas-node', () => ({
  WorkflowNode: () => {
    workflowNodeRender();
    return <div data-testid="workflow-node" />;
  },
}));

import { GraphCanvas } from './graph-canvas';

describe('GraphCanvas presence render isolation', () => {
  let renderer: ReactTestRenderer | null = null;

  beforeEach(() => {
    workflowNodeRender.mockClear();
    useWorkbenchStore.setState({ presenceByActor: {} });
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 404 })));
  });

  afterEach(async () => {
    if (renderer) await act(async () => renderer?.unmount());
    renderer = null;
    vi.unstubAllGlobals();
  });

  it('updates the connected overlay without rerendering workflow nodes', async () => {
    await act(async () => {
      renderer = create(
        <GraphCanvas
          graph={{ nodes: [node()], edges: [] }}
          outputs={[]}
          pendingProposal={null}
          run={run()}
          versionId="ver_a"
          workflowGraph={{ schema_version: 1, nodes: {}, edges: [] }}
          workspaceId="ws_a"
        />,
      );
      await flushActions();
    });
    const initialNodeRenders = workflowNodeRender.mock.calls.length;

    await act(async () => {
      useWorkbenchStore.getState().applyCanvasPresence({
        actor: { actorId: 'remote-performance-test', displayName: 'Remote' },
        cursor: { x: 50, y: 60 },
      });
      await flushActions();
    });

    expect(initialNodeRenders).toBeGreaterThan(0);
    expect(workflowNodeRender).toHaveBeenCalledTimes(initialNodeRenders);
    expect(renderer!.root.findByProps({ className: 'collab-cursor' }).children).toContain('Remote');
  });
});

function node(): GraphNodeState {
  return {
    id: 'video',
    nodeType: 'video.text_to_video',
    title: 'Video',
    category: 'Video',
    status: 'queued',
    position: { x: 20, y: 20 },
    provider: 'mock',
    summary: 'Video',
  };
}

function run(): NonNullable<WorkbenchState['run']> {
  return {
    id: 'run_a',
    label: 'Run',
    status: 'queued',
    steps: [{ nodeId: 'video', title: 'Video', state: 'queued', provider: 'mock' }],
    cost: { estimate: 0, actual: 0, currency: 'USD' },
  };
}
