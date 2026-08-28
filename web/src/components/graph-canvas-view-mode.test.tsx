import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { GraphNodeState, WorkbenchState } from '../types';
import { flushActions } from '../test-utils';
import { GraphCanvas } from './graph-canvas';
import { WorkflowNode } from './graph-canvas-node';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('GraphCanvas React Flow mode integration', () => {
  let renderer: ReactTestRenderer | null = null;
  const proposals: unknown[] = [];

  beforeEach(() => {
    proposals.length = 0;
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 404 })));
  });

  afterEach(async () => {
    if (renderer) await act(async () => renderer?.unmount());
    renderer = null;
    vi.unstubAllGlobals();
  });

  it('renders a read-only SSR fallback without mutation affordances', async () => {
    renderer = await renderCanvas(false);
    const node = renderer.root.findByType(WorkflowNode);

    expect(node.props.connectionDisabled).toBe(true);
    expect(node.props.resizable).toBe(false);
    expect(renderer.root.findByProps({ className: 'flow-view-controls nodrag nopan' })).toBeTruthy();
    expect(proposals).toEqual([]);
  });

  it('rejects delete and paste shortcuts in view mode with visible status', async () => {
    renderer = await renderCanvas(false);
    await act(async () => section().props.onKeyDown(keyEvent('Delete')));
    expect(statusText()).toBe('当前模式不允许删除');

    await act(async () => {
      section().props.onKeyDown(keyEvent('v', { metaKey: true }));
      await flushActions();
    });
    expect(statusText()).toBe('当前模式不允许粘贴');
    expect(proposals).toEqual([]);
  });

  it('keeps keyboard selection local in edit mode', async () => {
    renderer = await renderCanvas(true);
    const node = renderer.root.findByType(WorkflowNode);
    await act(async () => node.props.onKeyboardSelect(false));

    expect(renderer.root.findByType(WorkflowNode).props.selected).toBe(true);
    expect(proposals).toEqual([]);
  });

  it('switches from edit to view mode without retaining edit affordances', async () => {
    renderer = await renderCanvas(true);
    expect(renderer.root.findByType(WorkflowNode).props.resizable).toBe(true);

    await act(async () => renderer?.update(canvasElement(false)));

    expect(renderer.root.findByType(WorkflowNode).props.connectionDisabled).toBe(true);
    expect(renderer.root.findByType(WorkflowNode).props.resizable).toBe(false);
  });

  it('aborts both component-owned catalog requests on unmount', async () => {
    const signals: AbortSignal[] = [];
    vi.stubGlobal('fetch', vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
      if (init?.signal) signals.push(init.signal);
      return new Promise<Response>(() => undefined);
    }));
    renderer = await renderCanvas(true);

    expect(signals).toHaveLength(2);
    expect(signals.every((signal) => !signal.aborted)).toBe(true);
    await act(async () => renderer?.unmount());
    renderer = null;
    expect(signals.every((signal) => signal.aborted)).toBe(true);
  });

  async function renderCanvas(editable: boolean): Promise<ReactTestRenderer> {
    let next!: ReactTestRenderer;
    await act(async () => {
      next = create(canvasElement(editable));
      await flushActions();
    });
    return next;
  }

  function canvasElement(editable: boolean) {
    return (
      <GraphCanvas
        graph={{ nodes: [viewModeNode()], edges: [] }}
        onCreateProposal={editable ? async (proposal) => { proposals.push(proposal); } : undefined}
        outputs={[]}
        pendingProposal={null}
        run={run()}
        versionId="ver_a"
        workflowGraph={{ schema_version: 1, nodes: {}, edges: [] }}
        workspaceId="ws_a"
      />
    );
  }

  function section() {
    if (!renderer) throw new Error('renderer is not mounted');
    return renderer.root.findByProps({ className: 'p-canvas cv-bold' });
  }

  function statusText(): string {
    if (!renderer) throw new Error('renderer is not mounted');
    return renderer.root.findByProps({ className: 'canvas-status-toast' }).children.join('');
  }
});

function keyEvent(key: string, overrides: { metaKey?: boolean } = {}) {
  return {
    key,
    metaKey: overrides.metaKey ?? false,
    ctrlKey: false,
    target: null,
    preventDefault: vi.fn(),
  };
}

function viewModeNode(): GraphNodeState {
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
