import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { GraphNodeState, WorkbenchState } from '../types';
import { flushActions } from '../test-utils';
import { GraphCanvas } from './graph-canvas';
import { GraphCanvasToolbar } from './graph-canvas-toolbar';
import { WorkflowNode } from './graph-canvas-node';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('GraphCanvas view-mode integration', () => {
  let renderer: ReactTestRenderer | null = null;
  const onCreateProposal = vi.fn(async () => undefined);

  beforeEach(() => {
    onCreateProposal.mockClear();
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 404 })));
  });

  afterEach(async () => {
    if (renderer) await act(async () => renderer?.unmount());
    renderer = null;
    vi.unstubAllGlobals();
  });

  it('does not dispatch move, resize, or connection mutations in view mode', async () => {
    renderer = await renderCanvas();
    const workflowNode = renderer.root.findByType(WorkflowNode);
    const nodeEvent = pointerEvent();

    await act(async () => {
      workflowNode.props.onPointerDown(nodeEvent);
      workflowNode.props.onPointerMove({ ...nodeEvent, clientX: 80, clientY: 60 });
      workflowNode.props.onPointerUp({ ...nodeEvent, clientX: 80, clientY: 60 });
      workflowNode.props.onResizePointerDown(nodeEvent);
      workflowNode.props.onResizePointerMove({ ...nodeEvent, clientX: 90, clientY: 70 });
      workflowNode.props.onResizePointerUp({ ...nodeEvent, clientX: 90, clientY: 70 });
      workflowNode.props.onOutputPortPointerDown(
        viewModeNode(),
        { name: 'video', type: 'VIDEO' },
        0,
        nodeEvent,
      );
      section().props.onPointerUp(nodeEvent);
      await flushActions();
    });

    expect(onCreateProposal).not.toHaveBeenCalled();
    expect(workflowNode.props.connectionDisabled).toBe(true);
    expect(workflowNode.props.resizable).toBe(false);
  });

  it('rejects view-mode delete and paste shortcuts with visible status', async () => {
    renderer = await renderCanvas();

    await act(async () => {
      section().props.onKeyDown(keyEvent('Delete'));
    });
    expect(renderer.root.findByType(GraphCanvasToolbar).props.clipboardStatus).toBe(
      '当前模式不允许删除',
    );

    await act(async () => {
      section().props.onKeyDown(keyEvent('v', { metaKey: true }));
      await flushActions();
    });
    expect(renderer.root.findByType(GraphCanvasToolbar).props.clipboardStatus).toBe(
      '当前模式不允许粘贴',
    );
    expect(onCreateProposal).not.toHaveBeenCalled();
  });

  it('cancels an in-flight edit connection before a view-mode pointer completion', async () => {
    renderer = await renderCanvas();
    await act(async () => renderer?.root.findByType(GraphCanvasToolbar).props.setMode('edit'));
    const workflowNode = renderer.root.findByType(WorkflowNode);
    const event = pointerEvent();
    await act(async () => workflowNode.props.onOutputPortPointerDown(
      viewModeNode(),
      { name: 'video', type: 'VIDEO' },
      0,
      event,
    ));

    await act(async () => renderer?.root.findByType(GraphCanvasToolbar).props.setMode('view'));
    await act(async () => section().props.onPointerUp(event));

    expect(onCreateProposal).not.toHaveBeenCalled();
    expect(renderer.root.findByType(WorkflowNode).props.connectionDisabled).toBe(true);
  });

  it('dispatches an edit-mode node move through the extracted drag controller', async () => {
    renderer = await renderCanvas();
    await act(async () => renderer?.root.findByType(GraphCanvasToolbar).props.setMode('edit'));
    const workflowNode = renderer.root.findByType(WorkflowNode);
    const start = pointerEvent();

    await act(async () => {
      workflowNode.props.onPointerDown(start);
      workflowNode.props.onPointerMove({ ...start, clientX: 70, clientY: 50 });
      workflowNode.props.onPointerUp({ ...start, clientX: 70, clientY: 50 });
      await flushActions();
    });

    expect(onCreateProposal).toHaveBeenCalledWith(expect.objectContaining({
      baseVersionId: 'ver_a',
      ops: [expect.objectContaining({ id: 'video', op: 'move_node' })],
    }));
  });

  it('keeps an edit-mode connection rejection visible through the extracted controller', async () => {
    renderer = await renderCanvas();
    await act(async () => renderer?.root.findByType(GraphCanvasToolbar).props.setMode('edit'));
    class TestElement {}
    vi.stubGlobal('HTMLElement', TestElement);
    vi.stubGlobal('document', {
      elementFromPoint: () => ({
        closest: () => ({
          dataset: {
            portDirection: 'input',
            portIndex: '0',
            portName: 'video',
            portNodeId: 'save',
            portType: 'VIDEO',
          },
        }),
      }),
    });
    onCreateProposal.mockRejectedValueOnce(new Error('connection failed'));
    const workflowNode = renderer.root.findByType(WorkflowNode);
    const event = pointerEvent();

    await act(async () => {
      workflowNode.props.onOutputPortPointerDown(
        viewModeNode(),
        { name: 'video', type: 'VIDEO' },
        0,
        event,
      );
    });
    await act(async () => {
      section().props.onPointerUp(event);
      await flushActions();
    });

    expect(onCreateProposal).toHaveBeenCalledWith(expect.objectContaining({
      baseVersionId: 'ver_a',
      ops: [expect.objectContaining({ op: 'add_edge' })],
    }));
    expect(renderer.root.findByType(GraphCanvasToolbar).props.connectionStatus).toBe(
      'connection failed',
    );
  });

  async function renderCanvas(): Promise<ReactTestRenderer> {
    let next!: ReactTestRenderer;
    await act(async () => {
      next = create(
        <GraphCanvas
          graph={{ nodes: [viewModeNode()], edges: [] }}
          onCreateProposal={onCreateProposal}
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
    return next;
  }

  function section() {
    if (!renderer) throw new Error('renderer is not mounted');
    return renderer.root.findByProps({ className: 'p-canvas cv-bold' });
  }
});

function pointerEvent() {
  return {
    button: 0,
    clientX: 10,
    clientY: 10,
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    pointerId: 1,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
    currentTarget: {
      focus: vi.fn(),
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 600 }),
      hasPointerCapture: () => false,
      releasePointerCapture: vi.fn(),
      setPointerCapture: vi.fn(),
    },
  };
}

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
