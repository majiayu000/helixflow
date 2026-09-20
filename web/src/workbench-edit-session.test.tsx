import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './app';
import { composerDraftAfterSubmit } from './components/chat-pane';
import { useWorkbenchStore } from './store';
import type { WorkbenchState } from './types';
import {
  buildSetParamEditInput,
  deriveQueueLockReason,
  graphStateWithWorkflowLayout,
  partitionManualEditOps,
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
  imageProcessingJobs: [],
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

  it('persists canvas edits immediately as a new version', async () => {
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

  it('writes sequential canvas edits as separate versions', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_input, init) => {
        const body = JSON.parse(String(init?.body ?? '{}')) as { ops?: Array<{ op: string }> };
        const versionId = body.ops?.[0]?.op === 'set_param' ? 'ver_manual_2' : 'ver_manual_3';
        return jsonResponse({
          ...state,
          workspace: { ...state.workspace, versionId },
        });
      }),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().appendManualEdit({
      baseVersionId: 'ver_test_1',
      ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', value: 3 }],
    });
    const firstKey = JSON.parse(String(vi.mocked(fetch).mock.calls[0][1]?.body)).idempotencyKey as string;

    useWorkbenchStore.setState({
      state: {
        ...state,
        workspace: { ...state.workspace, versionId: 'ver_manual_2' },
      },
    });
    await useWorkbenchStore.getState().appendManualEdit({
      baseVersionId: 'ver_manual_2',
      ops: [{ op: 'add_edge', from: ['text', 'text'], to: ['video', 'prompt'], edge_type: 'text' }],
    });

    const secondKey = JSON.parse(String(vi.mocked(fetch).mock.calls[1][1]?.body)).idempotencyKey as string;
    expect(vi.mocked(fetch).mock.calls).toHaveLength(2);
    expect(secondKey).not.toBe(firstKey);
    expect(useWorkbenchStore.getState().editSession).toBeNull();
    expect(useWorkbenchStore.getState().state?.workspace.versionId).toBe('ver_manual_3');
  });

  it('persists spawned media cards through ops and their size through canvas snapshot', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith('/versions/ops')) {
        const body = JSON.parse(String(init?.body ?? '{}')) as {
          ops?: Array<{ op: string; id?: string; node_type?: string; title?: string; pos?: [number, number] }>;
        };
        const spawn = body.ops?.find((op) => op.op === 'spawn_node');
        return jsonResponse({
          ...state,
          workspace: { ...state.workspace, versionId: 'ver_manual_2' },
          graph: {
            ...state.graph,
            nodes: [
              ...state.graph.nodes,
              {
                id: spawn?.id ?? 'input_image',
                nodeType: spawn?.node_type ?? 'input.image',
                title: spawn?.title ?? 'Image',
                category: 'Input',
                status: 'queued',
                position: { x: spawn?.pos?.[0] ?? 80, y: spawn?.pos?.[1] ?? 80 },
                provider: null,
                summary: spawn?.node_type ?? 'input.image',
              },
            ],
          },
        });
      }
      if (url.endsWith('/canvas/snapshot')) {
        return jsonResponse({
          ...canvasForState({ ...state, workspace: { ...state.workspace, versionId: 'ver_manual_2' } }),
          revision: 1,
        });
      }
      return new Response('{}', { status: 404 });
    });
    vi.stubGlobal('fetch', fetchMock);
    useWorkbenchStore.getState().setInitialState(state);
    useWorkbenchStore.setState({ canvas: canvasForState(state) });

    await useWorkbenchStore.getState().appendManualEdit({
      baseVersionId: 'ver_test_1',
      ops: [
        {
          op: 'spawn_node',
          id: 'input_image_abc123',
          node_type: 'input.image',
          title: 'Image Input',
          params: {},
          pos: [80, 80],
        },
        { op: 'resize_node', id: 'input_image_abc123', size: [280, 280] },
      ],
    });

    expect(fetchMock.mock.calls.map(([input]) => String(input))).toEqual([
      '/api/workspaces/ws_test/versions/ops',
      '/api/workspaces/ws_test/canvas/snapshot',
    ]);
    expect(JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))).toMatchObject({
      ops: [{
        op: 'spawn_node',
        id: 'input_image_abc123',
        node_type: 'input.image',
      }],
    });
    expect(JSON.parse(String(fetchMock.mock.calls[1]?.[1]?.body))).toEqual({
      versionId: 'ver_manual_2',
      baseRevision: 0,
      sizes: [{ id: 'input_image_abc123', width: 280, height: 280 }],
    });
    expect(useWorkbenchStore.getState().editSession).toBeNull();
    expect(useWorkbenchStore.getState().state?.workspace.versionId).toBe('ver_manual_2');
    expect(useWorkbenchStore.getState().state?.graph.nodes.some((node) => node.id === 'input_image_abc123')).toBe(true);
  });

  it('rebases an edit queued while the preceding canvas version is saving', async () => {
    let resolveFirst!: (response: Response) => void;
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      if (fetchMock.mock.calls.length === 1) {
        return new Promise<Response>((resolve) => {
          resolveFirst = resolve;
        });
      }
      const body = JSON.parse(String(init?.body ?? '{}')) as {
        baseVersionId?: string;
        ops?: Array<{ op: string; prev?: unknown; value?: unknown }>;
      };
      expect(body).toMatchObject({
        baseVersionId: 'ver_manual_2',
        ops: [{ op: 'set_param', value: 6 }],
      });
      expect(body.ops?.[0]).not.toHaveProperty('prev');
      return jsonResponse({
        ...state,
        workspace: { ...state.workspace, versionId: 'ver_manual_3' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);
    useWorkbenchStore.getState().setInitialState(state);

    const first = useWorkbenchStore.getState().appendManualEdit({
      baseVersionId: 'ver_test_1',
      ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', prev: 4, value: 5 }],
    });
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    const second = useWorkbenchStore.getState().appendManualEdit({
      baseVersionId: 'ver_test_1',
      ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', prev: 4, value: 6 }],
    });

    resolveFirst(jsonResponse({
      ...state,
      workspace: { ...state.workspace, versionId: 'ver_manual_2' },
    }));

    await Promise.all([first, second]);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(useWorkbenchStore.getState().state?.workspace.versionId).toBe('ver_manual_3');
  });

  it('drops stale leftover edits instead of writing them onto a newer version', async () => {
    vi.stubGlobal('fetch', vi.fn());
    useWorkbenchStore.getState().setInitialState(state);
    const editSession = {
      baseVersionId: 'ver_stale',
      idempotencyKey: 'canvas_op_stale',
      source: 'user' as const,
      startedAt: '2026-07-11T00:00:00Z',
      ops: [{ op: 'move_node' as const, id: 'video', pos: [620, 210] as [number, number] }],
    };
    useWorkbenchStore.setState({ editSession });

    await expect(useWorkbenchStore.getState().commitManualEdits()).rejects.toThrow(
      '画布已更新到新版本',
    );

    expect(fetch).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().editSession).toBeNull();
    expect(useWorkbenchStore.getState().state?.chat.messages.at(-1)?.text).toContain(
      '画布已更新到新版本',
    );
  });

  it('discards leftover local preview without writing a new version', async () => {
    vi.stubGlobal('fetch', vi.fn());
    useWorkbenchStore.getState().setInitialState(state);
    useWorkbenchStore.setState({
      editSession: {
        baseVersionId: 'ver_test_1',
        idempotencyKey: 'canvas_op_preview',
        source: 'user',
        startedAt: '2026-07-11T00:00:00Z',
        ops: [{ op: 'move_node', id: 'video', pos: [620, 210] }],
      },
    });
    useWorkbenchStore.getState().discardManualEdits();

    expect(fetch).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().editSession).toBeNull();
    expect(useWorkbenchStore.getState().state?.graph.nodes.find((node) => node.id === 'video')?.position).toEqual({
      x: 486,
      y: 156,
    });
  });

  it('flushes leftover canvas edits before queue and agent requests', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith('/versions/ops')) {
        return jsonResponse({
          ...state,
          workspace: { ...state.workspace, versionId: 'ver_manual_2' },
        });
      }
      if (url.endsWith('/runs')) {
        return jsonResponse({
          run: {
            id: 'run_1',
            label: 'Run',
            status: 'queued',
            steps: [],
            cost: { estimate: 0, actual: 0, currency: 'USD' },
          },
          outputs: [],
          pendingConfirmation: null,
        });
      }
      if (url.endsWith('/messages')) {
        return jsonResponse({
          conversationId: 'conv_1',
          turnId: 'turn_1',
          turnStatus: 'succeeded',
          turnMode: 'chat',
          messages: [
            {
              id: 'msg_agent',
              role: 'agent',
              kind: 'chat',
              text: 'ok',
              time: '2026-07-11T00:00:00Z',
              turnId: 'turn_1',
            },
          ],
          proposal: null,
        });
      }
      return new Response('{}', { status: 404 });
    });
    vi.stubGlobal('fetch', fetchMock);
    useWorkbenchStore.getState().setInitialState(state);
    useWorkbenchStore.setState({
      editSession: {
        baseVersionId: 'ver_test_1',
        idempotencyKey: 'canvas_op_leftover',
        source: 'user',
        startedAt: '2026-07-11T00:00:00Z',
        ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', value: 3 }],
      },
    });

    await useWorkbenchStore.getState().queueRun();
    await expect(
      composerDraftAfterSubmit('运行当前 workflow', '运行当前 workflow', (text) =>
        useWorkbenchStore.getState().sendMessage(text),
      ),
    ).resolves.toBe('');

    expect(fetchMock.mock.calls.map(([input]) => String(input))).toEqual(expect.arrayContaining([
      '/api/workspaces/ws_test/versions/ops',
      '/api/workspaces/ws_test/runs',
      '/api/workspaces/ws_test/messages',
    ]));
    expect(useWorkbenchStore.getState().editSession).toBeNull();
    const messages = useWorkbenchStore.getState().state?.chat.messages ?? [];
    expect(messages.some((message) => message.text.includes('请先提交或放弃'))).toBe(false);
  });

  it('does not render a canvas commit dock for leftover local edits', () => {
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

    expect(markup).not.toContain('EDITING · 1 CHANGES');
    expect(markup).not.toContain('UNCOMMITTED');
    expect(markup).not.toContain('提交编辑');
    expect(markup).not.toContain('放弃');
    expect(markup).not.toContain('title="先提交或放弃 1 个手动编辑"');
  });

  it('splits layout ops out of versioned graph edits', () => {
    expect(partitionManualEditOps([
      {
        op: 'spawn_node',
        id: 'input_image_abc123',
        node_type: 'input.image',
        title: 'Image Input',
        params: {},
        pos: [80, 80],
      },
      { op: 'resize_node', id: 'input_image_abc123', size: [280, 280] },
      { op: 'move_node', id: 'video', pos: [620, 210] },
    ])).toEqual({
      graphOps: [{
        op: 'spawn_node',
        id: 'input_image_abc123',
        node_type: 'input.image',
        title: 'Image Input',
        params: {},
        pos: [80, 80],
      }],
      snapshot: {
        positions: [{ id: 'video', x: 620, y: 210 }],
        sizes: [{ id: 'input_image_abc123', width: 280, height: 280 }],
      },
    });
  });

  it('fills missing graph node sizes from the workflow graph', () => {
    const graph = graphStateWithWorkflowLayout(state.graph, {
      ...state.workflowGraph!,
      nodes: {
        ...state.workflowGraph!.nodes,
        video: {
          ...state.workflowGraph!.nodes.video,
          size: [320, 200],
        },
      },
    });
    expect(graph.nodes.find((node) => node.id === 'video')?.size).toEqual({
      width: 320,
      height: 200,
    });
    expect(state.graph.nodes.find((node) => node.id === 'video')?.size).toBeUndefined();
  });

  it('previews manual edit ops without mutating the committed state', () => {
    const preview = previewWorkbenchStateWithManualEdits(state, {
      baseVersionId: 'ver_test_1',
      idempotencyKey: 'canvas_op_existing',
      source: 'user',
      startedAt: '2026-07-02T00:00:00Z',
      ops: [
        { op: 'move_node', id: 'video', pos: [620, 210] },
        { op: 'resize_node', id: 'video', size: [260, 180] },
        { op: 'set_param', id: 'video', key: 'duration_sec', value: 3 },
      ],
    });

    expect(preview.graph.nodes.find((node) => node.id === 'video')?.position).toEqual({
      x: 620,
      y: 210,
    });
    expect(preview.graph.nodes.find((node) => node.id === 'video')?.size).toEqual({
      width: 260,
      height: 180,
    });
    expect(preview.workflowGraph?.nodes.video.params).toMatchObject({ duration_sec: 3 });
    expect(preview.workflowGraph?.nodes.video.size).toEqual([260, 180]);
    expect(state.graph.nodes.find((node) => node.id === 'video')?.position).toEqual({
      x: 486,
      y: 156,
    });
  });

  it('previews 宫格切分 spawn_node ops with source-to-tile lineage edges', () => {
    const photoState: WorkbenchState = {
      ...state,
      graph: {
        nodes: [
          {
            id: 'photo',
            nodeType: 'input.image',
            title: '产品主图',
            category: 'Input',
            status: 'succeeded',
            position: { x: 0, y: 0 },
            provider: null,
            summary: 'input.image',
          },
        ],
        edges: [],
      },
      workflowGraph: {
        schema_version: 1,
        nodes: {
          photo: {
            node_type: 'input.image',
            title: '产品主图',
            params: { storage_uri: 'upload://a' },
            pos: [0, 0],
          },
        },
        edges: [],
      },
    };
    const preview = previewWorkbenchStateWithManualEdits(photoState, {
      baseVersionId: 'ver_test_1',
      idempotencyKey: 'canvas_op_split',
      source: 'user',
      startedAt: '2026-07-02T00:00:00Z',
      ops: [
        {
          op: 'spawn_node',
          id: 'image_grid_r1c1',
          node_type: 'input.image',
          title: '产品主图 r1c1',
          params: { storage_uri: 'upload://t1' },
          pos: [400, 0],
          from: 'photo',
        },
        {
          op: 'spawn_node',
          id: 'image_grid_r1c2',
          node_type: 'input.image',
          title: '产品主图 r1c2',
          params: { storage_uri: 'upload://t2' },
          pos: [680, 0],
          from: 'photo',
        },
      ],
    });

    expect(preview.graph.nodes.map((node) => node.id).sort()).toEqual([
      'image_grid_r1c1',
      'image_grid_r1c2',
      'photo',
    ]);
    expect(preview.graph.edges).toEqual([
      {
        id: 'edge_photo_image_image_grid_r1c1_in_0',
        from: { nodeId: 'photo', port: 'image' },
        to: { nodeId: 'image_grid_r1c1', port: 'in' },
        kind: 'image',
      },
      {
        id: 'edge_photo_image_image_grid_r1c2_in_1',
        from: { nodeId: 'photo', port: 'image' },
        to: { nodeId: 'image_grid_r1c2', port: 'in' },
        kind: 'image',
      },
    ]);
    expect(preview.workflowGraph?.edges).toEqual([
      { from: ['photo', 'image'], to: ['image_grid_r1c1', 'in'], edge_type: 'image' },
      { from: ['photo', 'image'], to: ['image_grid_r1c2', 'in'], edge_type: 'image' },
    ]);
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

function canvasForState(nextState: WorkbenchState) {
  return {
    schemaVersion: 1,
    workspaceId: nextState.workspace.id,
    versionId: nextState.workspace.versionId,
    seq: 0,
    revision: 0,
    viewport: null,
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
