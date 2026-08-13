import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  CANVAS_PRESENCE_TTL_MS,
  LOCAL_CANVAS_ACTOR,
} from './canvas-presence';
import { composerDraftAfterSubmit } from './components/chat-pane';
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
    vi.useRealTimers();
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

  it('reactivates the rendered workspace when the next workspace fails to load', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ error: 'B unavailable' }), {
      status: 503,
      headers: { 'content-type': 'application/json' },
    })));
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));

    await useWorkbenchStore.getState().hydrate('ws_b');

    expect(useWorkbenchStore.getState().state?.workspace.id).toBe('ws_a');
    expect(useWorkbenchStore.getState().activeWorkspaceId).toBe('ws_a');
    expect(useWorkbenchStore.getState().status).toBe('error');
    expect(useWorkbenchStore.getState().error).toContain('workspace state request failed: 503');
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

  it('rejects a delayed workspace A message response without modifying B', async () => {
    let resolveMessage!: (response: Response) => void;
    const messageResponse = new Promise<Response>((resolve) => {
      resolveMessage = resolve;
    });
    vi.stubGlobal(
      'fetch',
      vi.fn(() => messageResponse),
    );
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const sending = composerDraftAfterSubmit('hello', 'hello', (text) =>
      useWorkbenchStore.getState().sendMessage(text),
    );
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
    await expect(sending).resolves.toBe('hello');

    expect(useWorkbenchStore.getState().state?.workspace.id).toBe('ws_b');
    expect(useWorkbenchStore.getState().state?.chat.messages).toHaveLength(0);
    expect(useWorkbenchStore.getState().error).toBeNull();
  });

  it('aborts a workspace upload and rejects its delayed completion after navigation', async () => {
    let resolveUpload!: (response: Response) => void;
    let uploadSignal: AbortSignal | null = null;
    vi.stubGlobal('fetch', vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
      uploadSignal = init?.signal as AbortSignal;
      return new Promise<Response>((resolve) => {
        resolveUpload = resolve;
      });
    }));
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const upload = useWorkbenchStore.getState().uploadImage(
      new File(['image'], 'sample.png', { type: 'image/png' }),
    );

    const workspaceB = stateForWorkspace('ws_b');
    useWorkbenchStore.getState().setInitialState(workspaceB);
    expect((uploadSignal as unknown as AbortSignal).aborted).toBe(true);
    resolveUpload(jsonResponse({
      id: 'upload_a',
      storageUri: 'workspace://uploads/upload_a/sample.png',
      filename: 'sample.png',
      mime: 'image/png',
    }));

    await expect(upload).rejects.toThrow('workspace changed');
    expect(useWorkbenchStore.getState().state).toEqual(workspaceB);
  });

  it('preserves the composer draft when the message request fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ error: 'provider unavailable' }), {
        status: 503,
        headers: { 'content-type': 'application/json' },
      })),
    );
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));

    await expect(
      composerDraftAfterSubmit('retry me', 'retry me', (text) =>
        useWorkbenchStore.getState().sendMessage(text),
      ),
    ).resolves.toBe('retry me');
    expect(useWorkbenchStore.getState().state?.chat.messages.at(-1)).toMatchObject({
      role: 'system',
      kind: 'run_failed',
      text: 'provider unavailable',
    });
  });

  it('rejects a stale proposal refresh without modifying B or clearing the draft', async () => {
    let resolveSnapshot!: (response: Response) => void;
    const snapshotResponse = new Promise<Response>((resolve) => {
      resolveSnapshot = resolve;
    });
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      if (String(input).endsWith('/messages')) {
        return Promise.resolve(jsonResponse({
          turnMode: 'create_workflow',
          messages: [{
            id: 'msg_applied_a',
            role: 'agent',
            kind: 'proposal_applied',
            text: 'applied A',
            time: 'unix:2',
          }],
          proposal: null,
          run: null,
          pendingConfirmation: null,
        }));
      }
      return snapshotResponse;
    });
    vi.stubGlobal('fetch', fetchMock);
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const sending = composerDraftAfterSubmit('apply it', 'apply it', (text) =>
      useWorkbenchStore.getState().sendMessage(text),
    );
    await waitUntil(() => fetchMock.mock.calls.some(([input]) => String(input).endsWith('/state')));

    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_b'));
    resolveSnapshot(jsonResponse(stateForWorkspace('ws_a')));

    await expect(sending).resolves.toBe('apply it');
    expect(useWorkbenchStore.getState().state?.workspace.id).toBe('ws_b');
    expect(useWorkbenchStore.getState().state?.chat.messages).toHaveLength(0);
    expect(useWorkbenchStore.getState().error).toBeNull();
  });

  it.each([
    ['queue', () => useWorkbenchStore.getState().queueRun()],
    ['interrupt', () => useWorkbenchStore.getState().interruptRun()],
    ['confirm', () => useWorkbenchStore.getState().confirmRun('run_ws_a')],
    ['hold', () => useWorkbenchStore.getState().holdRun('run_ws_a')],
  ])('rejects a stale %s completion without modifying B', async (_name, startAction) => {
    const response = delayedResponseFetch();
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const action = startAction();
    const workspaceB = stateForWorkspace('ws_b');
    useWorkbenchStore.getState().setInitialState(workspaceB);

    response.resolve(jsonResponse(runConfirmationForWorkspace('ws_a')));

    await expect(action).rejects.toThrow('workspace changed');
    expect(useWorkbenchStore.getState().state).toEqual(workspaceB);
    expect(useWorkbenchStore.getState().error).toBeNull();
  });

  it.each([
    ['undo', () => useWorkbenchStore.getState().undoVersion()],
    ['restore', () => useWorkbenchStore.getState().restoreVersion('ver_old_a')],
    ['layout', () => useWorkbenchStore.getState().saveLayout([{ id: 'video', x: 10, y: 20 }])],
    ['select output', () => useWorkbenchStore.getState().selectOutput('output_a')],
    ['accept output', () => useWorkbenchStore.getState().acceptOutput('output_a')],
    ['reject output', () => useWorkbenchStore.getState().rejectOutput('output_a')],
    ['create proposal', () => useWorkbenchStore.getState().createManualProposal({
      baseVersionId: 'ver_ws_a',
      ops: [{ op: 'move_node', id: 'video', pos: [10, 20] }],
    })],
    ['commit edits', () => {
      useWorkbenchStore.setState({
        editSession: {
          baseVersionId: 'ver_ws_a',
          idempotencyKey: 'canvas_op_a',
          source: 'user',
          startedAt: '2026-07-11T00:00:00Z',
          ops: [{ op: 'move_node', id: 'video', pos: [10, 20] }],
        },
      });
      return useWorkbenchStore.getState().commitManualEdits();
    }],
    ['apply proposal', () => useWorkbenchStore.getState().applyProposal('proposal_a')],
    ['dismiss proposal', () => useWorkbenchStore.getState().dismissProposal('proposal_a')],
  ])('rejects a stale %s state replacement without modifying B', async (_name, startAction) => {
    const response = delayedResponseFetch();
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const action = startAction();
    const workspaceB = stateForWorkspace('ws_b');
    useWorkbenchStore.getState().setInitialState(workspaceB);

    response.resolve(jsonResponse(stateForWorkspace('ws_a')));

    await expect(action).rejects.toThrow('workspace changed');
    expect(useWorkbenchStore.getState().state).toEqual(workspaceB);
    expect(useWorkbenchStore.getState().error).toBeNull();
  });

  it('does not append a stale A request failure to B', async () => {
    const response = delayedResponseFetch();
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const queueing = useWorkbenchStore.getState().queueRun();
    const workspaceB = stateForWorkspace('ws_b');
    useWorkbenchStore.getState().setInitialState(workspaceB);

    response.resolve(new Response(JSON.stringify({ error: 'A failed' }), {
      status: 503,
      headers: { 'content-type': 'application/json' },
    }));

    await expect(queueing).rejects.toThrow('workspace changed');
    expect(useWorkbenchStore.getState().state).toEqual(workspaceB);
    expect(useWorkbenchStore.getState().state?.chat.messages).toHaveLength(0);
    expect(useWorkbenchStore.getState().error).toBeNull();
  });

  it('does not revive an old A action generation after A to B to A', async () => {
    const response = delayedResponseFetch();
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    const restoring = useWorkbenchStore.getState().restoreVersion('ver_old_a');
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_b'));
    const currentA = stateForWorkspace('ws_a');
    currentA.workspace.versionId = 'ver_current_a';
    useWorkbenchStore.getState().setInitialState(currentA);

    const staleA = stateForWorkspace('ws_a');
    staleA.workspace.versionId = 'ver_stale_a';
    response.resolve(jsonResponse(staleA));

    await expect(restoring).rejects.toThrow('workspace changed');
    expect(useWorkbenchStore.getState().state).toEqual(currentA);
  });

  it('rejects a stale workflow export instead of returning A data in B', async () => {
    const response = delayedResponseFetch();
    const workspaceA = stateForWorkspace('ws_a');
    useWorkbenchStore.getState().setInitialState(workspaceA);
    const exporting = useWorkbenchStore.getState().exportWorkflow();
    const workspaceB = stateForWorkspace('ws_b');
    useWorkbenchStore.getState().setInitialState(workspaceB);

    response.resolve(jsonResponse(workspaceA.workflowGraph));

    await expect(exporting).rejects.toThrow('workspace changed');
    expect(useWorkbenchStore.getState().state).toEqual(workspaceB);
  });

  it('rejects a delayed workspace A provider response without modifying B', async () => {
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
    await expect(selecting).rejects.toThrow('workspace changed');

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

  it('rejects a delayed canvas comment response without modifying B', async () => {
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
    await expect(submitting).rejects.toThrow('workspace changed');

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

  it('does not store the current actor or its legacy echo as a collaborator', () => {
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));

    useWorkbenchStore.getState().applyCanvasPresence({
      actor: LOCAL_CANVAS_ACTOR,
      cursor: { x: 1, y: 2 },
    });
    useWorkbenchStore.getState().applyCanvasPresence({
      actor: { actorId: 'local', displayName: 'Local user' },
      cursor: { x: 3, y: 4 },
    });

    expect(useWorkbenchStore.getState().presenceByActor).toEqual({});
  });

  it('does not optimistically mirror outgoing presence into the collaborator store', async () => {
    let resolvePresence!: (response: Response) => void;
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>((resolve) => {
      resolvePresence = resolve;
    })));
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));

    const sending = useWorkbenchStore.getState().sendCanvasPresence({
      actor: LOCAL_CANVAS_ACTOR,
      cursor: { x: 1, y: 2 },
    });
    expect(useWorkbenchStore.getState().presenceByActor).toEqual({});

    resolvePresence(jsonResponse({ ok: true }));
    await sending;
    expect(useWorkbenchStore.getState().presenceByActor).toEqual({});
  });

  it('expires inactive collaborator presence', async () => {
    vi.useFakeTimers();
    useWorkbenchStore.getState().setInitialState(stateForWorkspace('ws_a'));
    useWorkbenchStore.getState().applyCanvasPresence({
      actor: { actorId: 'remote', displayName: 'Remote' },
      cursor: { x: 1, y: 2 },
    });

    expect(useWorkbenchStore.getState().presenceByActor).toHaveProperty('remote');
    await vi.advanceTimersByTimeAsync(CANVAS_PRESENCE_TTL_MS);
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
    expect(useWorkbenchStore.getState().state?.chat.messages).toHaveLength(0);
    expect(useWorkbenchStore.getState().error).toBeNull();

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
    await expect(useWorkbenchStore.getState().submitCanvasCommentOp({
      op: {
        op: 'comment_add',
        target: { kind: 'position', x: 1, y: 2 },
        body: 'stale A',
      },
    })).rejects.toThrow('workspace changed');
    await expect(useWorkbenchStore.getState().selectProvider('atlas')).rejects.toThrow(
      'workspace changed',
    );
    const urls = vi.mocked(fetch).mock.calls.map((call) => String(call[0]));
    expect(urls).not.toContain('/api/workspaces/ws_a/canvas/presence');
    expect(urls).not.toContain('/api/workspaces/ws_a/canvas/comments/ops');
    expect(urls).not.toContain('/api/workspaces/ws_a/provider');
    expect(useWorkbenchStore.getState().presenceByActor).toEqual({});
    expect(useWorkbenchStore.getState().error).toBeNull();

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

function delayedResponseFetch() {
  let resolve!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>((next) => {
    resolve = next;
  })));
  return { resolve: (response: Response) => resolve(response) };
}

function runConfirmationForWorkspace(workspaceId: string) {
  const state = stateForWorkspace(workspaceId);
  return {
    run: state.run,
    outputs: state.outputs,
    pendingConfirmation: state.pendingConfirmation,
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
