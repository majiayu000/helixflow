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
    expect(fetch).toHaveBeenCalledWith(
      '/api/workspaces/ws_test/state',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
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
    expect(fetch).toHaveBeenCalledWith(
      '/api/workspaces/ws_test/state',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  it('ignores a delayed workspace A snapshot after workspace B becomes active', async () => {
    const responses = delayedSnapshotFetch();
    const hydrateA = useWorkbenchStore.getState().hydrate('ws_a');
    const hydrateB = useWorkbenchStore.getState().hydrate('ws_b');

    responses.resolve('ws_b', stateForWorkspace('ws_b'));
    await hydrateB;
    responses.resolve('ws_a', stateForWorkspace('ws_a'));
    await hydrateA;

    expect(useWorkbenchStore.getState().state?.workspace.id).toBe('ws_b');
  });

  it('does not revive the first A generation after A to B to A', async () => {
    const responses = delayedSnapshotFetch();
    const firstA = useWorkbenchStore.getState().hydrate('ws_a');
    const b = useWorkbenchStore.getState().hydrate('ws_b');
    const secondA = useWorkbenchStore.getState().hydrate('ws_a');
    const staleA = stateForWorkspace('ws_a');
    staleA.run = { ...staleA.run!, id: 'run_stale_a' };
    const currentA = stateForWorkspace('ws_a');
    currentA.run = { ...currentA.run!, id: 'run_current_a' };

    responses.resolve('ws_b', stateForWorkspace('ws_b'));
    responses.resolve('ws_a', staleA);
    responses.resolve('ws_a', currentA);
    await Promise.all([firstA, b, secondA]);

    expect(useWorkbenchStore.getState().state?.workspace.id).toBe('ws_a');
    expect(useWorkbenchStore.getState().state?.run?.id).toBe('run_current_a');
  });

  it('ignores a delayed workspace A message response after switching to B', async () => {
    let resolveMessage!: (response: Response) => void;
    const messageResponse = new Promise<Response>((resolve) => {
      resolveMessage = resolve;
    });
    vi.stubGlobal(
      'fetch',
      vi.fn(() => messageResponse),
    );
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const sending = useWorkbenchStore.getState().sendMessage('hello');
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_b'));
    resolveMessage(
      jsonResponse({
        turnMode: 'chat',
        messages: [
          {
            id: 'msg_late_a',
            role: 'agent',
            kind: 'chat',
            text: 'late A response',
            time: 'unix:1',
          },
        ],
        proposal: null,
      }),
    );
    await sending;

    expect(useWorkbenchStore.getState().state?.workspace.id).toBe('ws_b');
    expect(useWorkbenchStore.getState().state?.chat.messages).toHaveLength(0);
  });

  it('ignores a delayed workspace A provider response after switching to B', async () => {
    let resolveProvider!: (response: Response) => void;
    const providerResponse = new Promise<Response>((resolve) => {
      resolveProvider = resolve;
    });
    vi.stubGlobal('fetch', vi.fn(() => providerResponse));
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const selecting = useWorkbenchStore.getState().selectProvider('atlas');
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_b'));
    const staleA = stateForWorkspace('ws_a');
    staleA.providers.selectedProvider = 'atlas';
    resolveProvider(jsonResponse(staleA));
    await selecting;

    expect(useWorkbenchStore.getState().state?.workspace.id).toBe('ws_b');
    expect(useWorkbenchStore.getState().state?.providers.selectedProvider).toBe('mock');
  });

  it('runs a trailing refresh when an event arrives during an in-flight refresh', async () => {
    const responses = delayedSnapshotFetch();
    useWorkbenchStore.getState().setInitialState(stateWithRun('run_1', 'running', 0));
    useWorkbenchStore.getState().setConnection('live');
    useWorkbenchStore.getState().applyEvent(runEvent('run_1', 2, 'run.succeeded'));

    responses.resolve('ws_test', stateWithRun('run_1', 'running', 0));
    await waitUntil(() => vi.mocked(fetch).mock.calls.length === 4);
    responses.resolve('ws_test', stateWithRun('run_1', 'succeeded', 1));
    await waitUntil(() => useWorkbenchStore.getState().state?.outputs.length === 1);

    expect(useWorkbenchStore.getState().state?.run?.status).toBe('succeeded');
    expect(fetch).toHaveBeenCalledTimes(4);
  });

  it('ignores a delayed canvas comment response after switching workspaces', async () => {
    let resolveCanvas!: (response: Response) => void;
    const canvasResponse = new Promise<Response>((resolve) => {
      resolveCanvas = resolve;
    });
    vi.stubGlobal('fetch', vi.fn(() => canvasResponse));
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const submitting = useWorkbenchStore.getState().submitCanvasCommentOp({
      baseSeq: 0,
      op: {
        op: 'comment_add',
        target: { kind: 'position', x: 10, y: 20 },
        body: 'workspace A comment',
      },
    });
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_b'));
    resolveCanvas(jsonResponse(canvasForState(stateForWorkspace('ws_a'))));
    await submitting;

    expect(useWorkbenchStore.getState().state?.workspace.id).toBe('ws_b');
    expect(useWorkbenchStore.getState().canvas).toBeNull();
  });

  it('clears old presence and selection when a new workspace activates', () => {
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    useWorkbenchStore.getState().setCanvasSelection(['video']);
    useWorkbenchStore.getState().applyCanvasPresence({
      actor: { actorId: 'actor_a', displayName: 'A' },
      selection: { nodeIds: ['video'], edgeIds: [] },
    });

    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_b'));

    expect(useWorkbenchStore.getState().selectedCanvasNodeIds).toEqual([]);
    expect(useWorkbenchStore.getState().presenceByActor).toEqual({});
  });

  it('ignores a delayed presence failure from the prior workspace', async () => {
    let rejectPresence!: (error: Error) => void;
    const presenceResponse = new Promise<Response>((_resolve, reject) => {
      rejectPresence = reject;
    });
    vi.stubGlobal('fetch', vi.fn(() => presenceResponse));
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    useWorkbenchStore.setState({ canvasConnection: 'live' });
    const sending = useWorkbenchStore.getState().sendCanvasPresence({
      actor: { actorId: 'actor_a', displayName: 'A' },
      cursor: { x: 1, y: 2 },
    });
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_b'));
    rejectPresence(new Error('late A failure'));
    await sending;

    expect(useWorkbenchStore.getState().canvasConnection).toBe('live');
    expect(useWorkbenchStore.getState().presenceByActor).toEqual({});
  });

  it('publishes generation changes so A to B to A can resubscribe', () => {
    const generations: number[] = [];
    const unsubscribe = useWorkbenchStore.subscribe((store) => {
      if (generations.at(-1) !== store.workspaceGeneration) {
        generations.push(store.workspaceGeneration);
      }
    });
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_b'));
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    unsubscribe();

    expect(generations.slice(-3)).toEqual([
      expect.any(Number),
      expect.any(Number),
      expect.any(Number),
    ]);
    expect(new Set(generations.slice(-3)).size).toBe(3);
  });

  it('does not post from stale visible state after target activation begins', async () => {
    const responses = delayedSnapshotFetch();
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const hydrateB = useWorkbenchStore.getState().hydrate('ws_b');

    await expect(
      useWorkbenchStore.getState().sendMessage('must not post to A'),
    ).rejects.toThrow('workspace changed');
    expect(vi.mocked(fetch).mock.calls.map((call) => String(call[0]))).not.toContain(
      '/api/workspaces/ws_a/messages',
    );
    expect(useWorkbenchStore.getState().state?.chat.messages.at(-1)).toMatchObject({
      role: 'system',
      kind: 'run_failed',
    });

    responses.resolve('ws_b', stateForWorkspace('ws_b'));
    await hydrateB;
  });

  it('does not post presence, comments, or provider writes from stale visible state', async () => {
    const responses = delayedSnapshotFetch();
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const hydrateB = useWorkbenchStore.getState().hydrate('ws_b');

    await useWorkbenchStore.getState().sendCanvasPresence({
      actor: { actorId: 'actor_a', displayName: 'A' },
      cursor: { x: 1, y: 2 },
    });
    await useWorkbenchStore.getState().submitCanvasCommentOp({
      op: {
        op: 'comment_add',
        target: { kind: 'position', x: 1, y: 2 },
        body: 'stale A',
      },
    });
    await useWorkbenchStore.getState().selectProvider('atlas');
    const urls = vi.mocked(fetch).mock.calls.map((call) => String(call[0]));
    expect(urls).not.toContain('/api/workspaces/ws_a/canvas/presence');
    expect(urls).not.toContain('/api/workspaces/ws_a/canvas/comments/ops');
    expect(urls).not.toContain('/api/workspaces/ws_a/provider');
    expect(useWorkbenchStore.getState().presenceByActor).toEqual({});

    responses.resolve('ws_b', stateForWorkspace('ws_b'));
    await hydrateB;
  });
});

function stateForWorkspace(workspaceId: string): WorkbenchState {
  const state = stateWithRun(`run_${workspaceId}`, 'running', 0);
  return {
    ...state,
    workspace: {
      ...state.workspace,
      id: workspaceId,
      versionId: `ver_${workspaceId}`,
    },
  };
}

function delayedSnapshotFetch() {
  const pending = new Map<string, Array<(response: Response) => void>>();
  vi.stubGlobal(
    'fetch',
    vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      return new Promise<Response>((resolve) => {
        const queue = pending.get(url) ?? [];
        queue.push(resolve);
        pending.set(url, queue);
      });
    }),
  );
  return {
    resolve(workspaceId: string, state: WorkbenchState) {
      const stateUrl = `/api/workspaces/${workspaceId}/state`;
      const canvasUrl = `/api/workspaces/${workspaceId}/canvas`;
      pending.get(stateUrl)?.shift()?.(jsonResponse(state));
      pending.get(canvasUrl)?.shift()?.(jsonResponse(canvasForState(state)));
    },
  };
}

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
