import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useWorkbenchStore } from './store';
import type { WorkbenchState } from './types';

describe('background run state reconciliation', () => {
  beforeEach(() => {
    useWorkbenchStore.setState({
      status: 'idle',
      error: null,
      connection: 'offline',
      state: null,
      editSession: null,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('refetches workspace state after terminal run events', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(stateWithRun('run_1', 'succeeded', 1))));
    useWorkbenchStore.getState().setInitialState(stateWithRun('run_1', 'running', 0));

    useWorkbenchStore.getState().applyEvent(runEvent('run_1', 2, 'run.succeeded'));

    await waitUntil(() => useWorkbenchStore.getState().state?.outputs.length === 1);
    expect(fetch).toHaveBeenCalledWith('/api/workspaces/ws_test/state');
    expect(useWorkbenchStore.getState().state?.run?.status).toBe('succeeded');
  });

  it('refetches workspace state when an event belongs to a different run', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(stateWithRun('run_new', 'running', 0))));
    useWorkbenchStore.getState().setInitialState(stateWithRun('run_old', 'running', 0));

    useWorkbenchStore.getState().applyEvent({
      ...runEvent('run_new', 1, 'node.state'),
      data: { node_id: 'video', state: 'running' },
    });

    await waitUntil(() => useWorkbenchStore.getState().state?.run?.id === 'run_new');
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it('refetches workspace state when the websocket reconnects', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(stateWithRun('run_1', 'interrupted', 0))));
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

function runEvent(runId: string, seq: number, ev: string) {
  return {
    workspace_id: 'ws_test',
    run_id: runId,
    seq,
    server_time: '2026-07-02T00:00:01Z',
    ev,
    data: {},
  };
}

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

async function waitUntil(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  throw new Error('condition was not met');
}
