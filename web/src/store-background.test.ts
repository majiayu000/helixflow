import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useWorkbenchStore } from './store';
import { jsonResponse, runEvent, waitUntil } from './test-utils';
import type { WorkbenchState } from './types';

describe('background run state reconciliation', () => {
  beforeEach(() => {
    useWorkbenchStore.setState({
      status: 'idle',
      error: null,
      connection: 'offline',
      canvasStatus: 'idle',
      canvasError: null,
      canvasConnection: 'offline',
      canvas: null,
      selectedCanvasNodeIds: [],
      presenceByActor: {},
      state: null,
      editSession: null,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('refetches workspace state after terminal run events', async () => {
    mockSnapshotFetch(stateWithRun('run_1', 'succeeded', 1));
    useWorkbenchStore.getState().setInitialState(stateWithRun('run_1', 'running', 0));

    useWorkbenchStore.getState().applyEvent(runEvent('run_1', 2, 'run.succeeded'));

    await waitUntil(() => useWorkbenchStore.getState().state?.outputs.length === 1);
    expect(fetch).toHaveBeenCalledWith('/api/workspaces/ws_test/state');
    expect(useWorkbenchStore.getState().state?.run?.status).toBe('succeeded');
  });

  it('refetches workspace state when an event belongs to a different run', async () => {
    mockSnapshotFetch(stateWithRun('run_new', 'running', 0));
    useWorkbenchStore.getState().setInitialState(stateWithRun('run_old', 'running', 0));

    useWorkbenchStore.getState().applyEvent({
      ...runEvent('run_new', 1, 'node.state'),
      data: { node_id: 'video', state: 'running' },
    });

    await waitUntil(() => useWorkbenchStore.getState().state?.run?.id === 'run_new');
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it('keeps retry notices after the retry snapshot replaces the run', async () => {
    mockSnapshotFetch(stateWithRun('run_child', 'running', 0));
    useWorkbenchStore.getState().setInitialState(stateWithRun('run_parent', 'failed', 0));

    useWorkbenchStore.getState().applyEvent({
      ...runEvent('run_parent', 3, 'run.retry'),
      data: {
        child_run_id: 'run_child',
        attempt: 1,
        requires_confirmation: false,
      },
    });

    await waitUntil(() => useWorkbenchStore.getState().state?.run?.id === 'run_child');
    expect(useWorkbenchStore.getState().state?.chat.messages.at(-1)?.text).toContain(
      'started automatically',
    );
  });

  it('refetches workspace state when the websocket reconnects', async () => {
    mockSnapshotFetch(stateWithRun('run_1', 'interrupted', 0));
    useWorkbenchStore.getState().setInitialState(stateWithRun('run_1', 'running', 0));

    useWorkbenchStore.getState().setConnection('live');

    await waitUntil(() => useWorkbenchStore.getState().state?.run?.status === 'interrupted');
    expect(fetch).toHaveBeenCalledWith('/api/workspaces/ws_test/state');
  });
});

function stateWithRun(
  runId: string,
  status: NonNullable<WorkbenchState['run']>['status'],
  outputCount: number,
): WorkbenchState {
  return {
    eventSeq: 0,
    workspace: {
      id: 'ws_test',
      name: 'Test Workspace',
      versionId: 'ver_test',
      updatedAt: '2026-07-02T00:00:00Z',
    },
    providers: {
      defaultProvider: 'mock',
      selectedProvider: 'mock',
      runtimeProviders: [],
      workflowBackends: [],
      apiConnectors: [],
    },
    chat: { messages: [] },
    graph: {
      nodes: [
        {
          id: 'video',
          nodeType: 'video.text_to_video',
          title: 'Video',
          category: 'Video',
          status: status === 'succeeded' ? 'succeeded' : 'running',
          position: { x: 0, y: 0 },
          provider: 'mock',
          summary: 'Video',
        },
      ],
      edges: [],
    },
    run: {
      id: runId,
      label: 'Run',
      status,
      steps: [{ nodeId: 'video', title: 'Video', state: 'running', provider: 'mock' }],
      cost: { estimate: 0, actual: 0, currency: 'USD' },
    },
    outputs: Array.from({ length: outputCount }, (_, index) => ({
      id: `out_${index}`,
      kind: 'video',
      title: 'Video',
      storageUri: `artifacts/${index}.mp4`,
      selected: index === 0,
      meta: '{}',
      mime: 'video/mp4',
      preview: {
        kind: 'video' as const,
        content: `/api/artifacts/out_${index}/content`,
        mime: 'video/mp4',
      },
    })),
    history: [],
    pendingConfirmation: null,
    pendingProposal: null,
    workflowGraph: { schema_version: 1, nodes: {}, edges: [] },
  };
}

function mockSnapshotFetch(nextState: WorkbenchState) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url === '/api/workspaces/ws_test/state') {
        return jsonResponse(nextState);
      }
      if (url === '/api/workspaces/ws_test/canvas') {
        return jsonResponse(canvasForState(nextState));
      }
      return new Response('{}', { status: 404 });
    }),
  );
}

function canvasForState(nextState: WorkbenchState) {
  return {
    schemaVersion: 1,
    workspaceId: nextState.workspace.id,
    versionId: nextState.workspace.versionId,
    seq: 0,
    nodes: nextState.graph.nodes.map((node) => ({
      id: node.id,
      nodeType: node.nodeType,
      title: node.title,
      position: node.position,
      params: {},
      runtime: null,
      metadata: { source: 'test' },
    })),
    edges: nextState.graph.edges,
    comments: [],
    runtime: {},
    metadata: { source: 'test' },
  };
}
