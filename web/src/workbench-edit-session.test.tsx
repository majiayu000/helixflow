import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './app';
import { useWorkbenchStore } from './store';
import type { WorkbenchState } from './types';
import {
  buildSetParamEditInput,
  deriveQueueLockReason,
  previewWorkbenchStateWithManualEdits,
} from './workbench-edit-session';

const state: WorkbenchState = {
  eventSeq: 0,
  workspace: {
    id: 'ws_test',
    name: 'Test Workspace',
    versionId: 'ver_test_1',
    updatedAt: '2026-07-02T00:00:00Z',
  },
  providers: {
    defaultProvider: 'mock',
    selectedProvider: 'mock',
    runtimeProviders: [
      {
        id: 'mock',
        label: 'Mock Provider',
        kind: 'local_test',
        enabled: true,
        status: 'healthy',
        message: 'mock provider ready',
        capabilities: ['text_to_video'],
      },
    ],
    workflowBackends: [],
    apiConnectors: [],
  },
  chat: { messages: [] },
  graph: {
    nodes: [
      {
        id: 'text',
        nodeType: 'input.text',
        title: 'Text',
        category: 'Input',
        status: 'succeeded',
        position: { x: 48, y: 158 },
        provider: null,
        summary: 'Source copy',
      },
      {
        id: 'video',
        nodeType: 'video.text_to_video',
        title: 'Video',
        category: 'Video',
        status: 'queued',
        position: { x: 486, y: 156 },
        provider: 'mock',
        summary: '9:16, 4 seconds',
      },
    ],
    edges: [
      {
        id: 'edge_text_video',
        from: { nodeId: 'text', port: 'text' },
        to: { nodeId: 'video', port: 'prompt' },
        kind: 'text',
      },
    ],
  },
  run: null,
  outputs: [],
  history: [],
  pendingConfirmation: null,
  pendingProposal: null,
  workflowGraph: {
    schema_version: 1,
    nodes: {
      text: {
        node_type: 'input.text',
        title: 'Text',
        params: { text: 'Create a vertical product teaser.' },
        pos: [48, 158],
      },
      video: {
        node_type: 'video.text_to_video',
        title: 'Video',
        params: { prompt: 'clean product shot', duration_sec: 4, aspect_ratio: '9:16' },
        pos: [486, 156],
      },
    },
    edges: [{ from: ['text', 'text'], to: ['video', 'prompt'], edge_type: 'text' }],
  },
};

describe('manual edit session workbench flow', () => {
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
    vi.restoreAllMocks();
  });

  it('keeps manual edits uncommitted until the user commits the edit session', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          ...state,
          workspace: { ...state.workspace, versionId: 'ver_manual_2' },
          graph: {
            ...state.graph,
            nodes: state.graph.nodes.map((node) =>
              node.id === 'video' ? { ...node, summary: 'duration_sec: 3' } : node,
            ),
          },
        }),
      ),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().appendManualEdit({
      baseVersionId: 'ver_test_1',
      ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', value: 3 }],
    });

    expect(fetch).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().editSession?.ops).toHaveLength(1);
    expect(useWorkbenchStore.getState().state?.workspace.versionId).toBe('ver_test_1');

    await useWorkbenchStore.getState().commitManualEdits();

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/ws_test/versions/ops', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: expect.any(String),
    });
    expect(JSON.parse(String(fetchMock.mock.calls[0][1]?.body))).toMatchObject({
      baseVersionId: 'ver_test_1',
      idempotencyKey: expect.any(String),
      label: 'Manual edit session (1 changes)',
      ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', value: 3 }],
    });
    expect(useWorkbenchStore.getState().editSession).toBeNull();
    expect(useWorkbenchStore.getState().state?.workspace.versionId).toBe('ver_manual_2');
  });

  it('reuses one idempotency key for retries within the same edit session', async () => {
    vi.stubGlobal('fetch', vi.fn());
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().appendManualEdit({
      baseVersionId: 'ver_test_1',
      ops: [{ op: 'move_node', id: 'video', pos: [620, 210] }],
    });
    const firstKey = useWorkbenchStore.getState().editSession?.idempotencyKey;

    await useWorkbenchStore.getState().appendManualEdit({
      baseVersionId: 'ver_test_1',
      ops: [{ op: 'add_edge', from: ['text', 'text'], to: ['video', 'prompt'], edge_type: 'text' }],
    });

    const editSession = useWorkbenchStore.getState().editSession;
    expect(editSession?.idempotencyKey).toBe(firstKey);
    expect(editSession?.ops).toHaveLength(2);
  });

  it('discards manual edits without writing a new version', async () => {
    vi.stubGlobal('fetch', vi.fn());
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().appendManualEdit({
      baseVersionId: 'ver_test_1',
      ops: [{ op: 'move_node', id: 'video', pos: [620, 210] }],
    });
    useWorkbenchStore.getState().discardManualEdits();

    expect(fetch).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().editSession).toBeNull();
    expect(useWorkbenchStore.getState().state?.graph.nodes.find((node) => node.id === 'video')?.position).toEqual({
      x: 486,
      y: 156,
    });
  });

  it('locks queue and agent requests while manual edits are dirty', async () => {
    vi.stubGlobal('fetch', vi.fn());
    useWorkbenchStore.getState().setInitialState(state);
    await useWorkbenchStore.getState().appendManualEdit({
      baseVersionId: 'ver_test_1',
      ops: [{ op: 'move_node', id: 'video', pos: [620, 210] }],
    });

    await useWorkbenchStore.getState().queueRun();
    await useWorkbenchStore.getState().sendMessage('运行当前 workflow');

    expect(fetch).not.toHaveBeenCalled();
    const messages = useWorkbenchStore.getState().state?.chat.messages ?? [];
    expect(messages.at(-2)?.text).toContain('请先提交或放弃手动编辑后再运行 Queue');
    expect(messages.at(-1)?.text).toContain('请先提交或放弃手动编辑后再发送 Agent 请求');
  });

  it('renders dirty edit summary and queue lock reason in the workbench shell', () => {
    const markup = renderToStaticMarkup(
      <App
        initialEditSession={{
          baseVersionId: 'ver_test_1',
          idempotencyKey: 'canvas_op_existing',
          source: 'user',
          startedAt: '2026-07-02T00:00:00Z',
          ops: [{ op: 'move_node', id: 'video', pos: [620, 210] }],
        }}
        initialState={state}
      />,
    );

    expect(markup).toContain('EDITING · 1 CHANGES');
    expect(markup).toContain('Move video to 620, 210');
    expect(markup).toContain('title="先提交或放弃 1 个手动编辑"');
    expect(markup).toContain('Commit');
    expect(markup).toContain('Discard');
  });

  it('previews manual edit ops without mutating the committed state', () => {
    const preview = previewWorkbenchStateWithManualEdits(state, {
      baseVersionId: 'ver_test_1',
      idempotencyKey: 'canvas_op_existing',
      source: 'user',
      startedAt: '2026-07-02T00:00:00Z',
      ops: [
        { op: 'move_node', id: 'video', pos: [620, 210] },
        { op: 'set_param', id: 'video', key: 'duration_sec', value: 3 },
      ],
    });

    expect(preview.graph.nodes.find((node) => node.id === 'video')?.position).toEqual({
      x: 620,
      y: 210,
    });
    expect(preview.workflowGraph?.nodes.video.params).toMatchObject({ duration_sec: 3 });
    expect(state.graph.nodes.find((node) => node.id === 'video')?.position).toEqual({
      x: 486,
      y: 156,
    });
  });

  it('builds inspector set_param edits with prev from the preview workflow graph', () => {
    expect(
      buildSetParamEditInput(
        'ver_test_1',
        state.workflowGraph,
        'video',
        'duration_sec',
        6,
      ),
    ).toEqual({
      baseVersionId: 'ver_test_1',
      label: 'Inspector set duration_sec',
      ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', prev: 4, value: 6 }],
    });
  });

  it('derives a single queue lock reason for dirty edits', () => {
    expect(
      deriveQueueLockReason({
        activeRun: false,
        busy: false,
        dirtyEditCount: 2,
        graphNodeCount: 2,
        hasProviderNodes: true,
        pendingConfirmation: false,
        pendingProposal: false,
        providerMessage: 'ready',
        providerReady: true,
      }),
    ).toEqual({ kind: 'dirty_edits', count: 2 });
  });
});

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}
