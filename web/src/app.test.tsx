import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './app';
import { ArtifactStage } from './components/artifact-stage';
import { shouldSubmitComposerKey } from './components/chat-pane';
import {
  DEFAULT_GRAPH_VIEW,
  GraphCanvas,
  computeMinimapLayout,
  loadGraphCanvasView,
  minimapViewportRect,
  saveGraphCanvasView,
  viewForMinimapPoint,
  viewStorageKey,
  zoomViewAtPoint,
} from './components/graph-canvas';
import {
  applyPositionDrafts,
  moveNodeDrafts,
  positionUpdatesFromDrafts,
  selectionForNodePointer,
} from './components/graph-canvas-layout';
import {
  GraphSelectionInspector,
} from './components/graph-canvas-inspector';
import {
  buildComparableNodeMap,
  buildEdgeSignatureSet,
  buildNodeMap,
  buildRunStepStateMap,
  nodeDiffState,
} from './components/graph-canvas-rendering';
import {
  ManualProposalPanel,
  buildManualProposalInput,
  defaultParamsForDefinition,
  parseManualJson,
} from './components/manual-proposal-panel';
import {
  fitViewToNodes,
  graphShortcutFromEvent,
  mergeSelection,
  sanitizeClipboardText,
  selectedIdsInWorldRect,
  selectionClipboardText,
  selectionRectFromPoints,
  worldRectFromLocalRect,
} from './components/graph-canvas-selection';
import { ConfirmModal, HistoryPanel } from './components/run-panels';
import { TopBar } from './components/top-bar';
import { applyRunEvent, useWorkbenchStore } from './store';
import { CanvasMessageContextSchema } from './types';
import type { NodeCatalog, WorkbenchState } from './types';

const state: WorkbenchState = {
  eventSeq: 0,
  workspace: {
    id: 'ws_test',
    name: 'Test Workspace',
    versionId: 'ver_test_1',
    updatedAt: '2026-06-12T00:00:00Z',
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
        capabilities: ['prompt_writer', 'image_generate', 'text_to_video'],
      },
    ],
    workflowBackends: [{ id: 'helixflow_graph', label: 'Helixflow Graph', status: 'healthy' }],
    apiConnectors: [
      { id: 'mock.prompt_writer', provider: 'mock', capability: 'prompt_writer', status: 'healthy' },
      { id: 'mock.image_generate', provider: 'mock', capability: 'image_generate', status: 'healthy' },
      { id: 'mock.text_to_video', provider: 'mock', capability: 'text_to_video', status: 'healthy' },
    ],
  },
  chat: {
    messages: [
      {
        id: 'msg_1',
        role: 'user',
        text: 'Create a vertical product teaser.',
        time: '09:10',
      },
    ],
  },
  graph: {
    nodes: [
      {
        id: 'text',
        nodeType: 'input.text',
        title: 'Launch note',
        category: 'Input',
        status: 'succeeded',
        position: { x: 48, y: 158 },
        provider: null,
        summary: 'Source copy',
      },
      {
        id: 'video',
        nodeType: 'video.text_to_video',
        title: 'Video render',
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
  run: {
    id: 'run_test_1',
    label: 'Manual preview',
    status: 'running',
    steps: [
      { nodeId: 'text', title: 'Launch note', state: 'succeeded', provider: null },
      { nodeId: 'video', title: 'Video render', state: 'queued', provider: 'mock' },
    ],
    cost: { estimate: 0, actual: 0, currency: 'USD' },
  },
  outputs: [
    {
      id: 'art_video_1',
      kind: 'video',
      title: 'Vertical teaser',
      storageUri: 'workspace://outputs/run_test_1/video/text_to_video.mp4',
      selected: true,
      meta: '1080 x 1920',
    },
  ],
  history: [
    {
      id: 'hist_1',
      kind: 'version',
      label: 'Initial graph',
      time: '09:05',
      summary: 'Version ver_test_1',
    },
  ],
  pendingConfirmation: {
    id: 'confirm_1',
    title: 'Agent requested run',
    summary: 'Run the current graph.',
    cost: { amount: 0, currency: 'USD' },
  },
  pendingProposal: null,
  workflowGraph: {
    schema_version: 1,
    nodes: {
      text: {
        node_type: 'input.text',
        title: 'Launch note',
        params: { text: 'Create a vertical product teaser.' },
        pos: [48, 158],
      },
      video: {
        node_type: 'video.text_to_video',
        title: 'Video render',
        params: {
          prompt: 'clean product shot',
          duration_sec: 4,
          aspect_ratio: '9:16',
        },
        pos: [486, 156],
      },
    },
    edges: [
      {
        from: ['text', 'text'],
        to: ['video', 'prompt'],
        edge_type: 'text',
      },
    ],
  },
};

describe('App', () => {
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

  it('renders the primary workbench layout from backend state', () => {
    const markup = renderToStaticMarkup(<App initialState={state} />);

    expect(markup).toContain('helixflow');
    expect(markup).toContain('Test Workspace');
    expect(markup).toContain('对话');
    expect(markup).toContain('2 节点');
    expect(markup).toContain('Mock Provider');
    expect(markup).toContain('本地测试');
    expect(markup).toContain('运行中');
    expect(markup).toContain('1 个真实 artifact');
    expect(markup).toContain('版本与运行历史');
    expect(markup).toContain('Agent requested run');
  });

  it('renders seed sweep confirmation metadata', () => {
    const markup = renderToStaticMarkup(
      <ConfirmModal
        busy={false}
        confirmation={{
          id: 'run_seed_404',
          title: 'Seed sweep run plan',
          summary: 'Seed sweep is waiting for confirmation (4 runs).',
          cost: { amount: 1.68, currency: 'USD' },
          runCount: 4,
          pendingChanges: ['video.seed = 101', 'video.seed = 202'],
          interruptible: true,
        }}
        onApprove={async () => {}}
        onHold={async () => {}}
      />,
    );

    expect(markup).toContain('4 次运行');
    expect(markup).toContain('video.seed = 101');
    expect(markup).toContain('执行期间可中断');
    expect(markup).toContain('1.68 USD');
  });

  it('keeps the interrupt button enabled while another run action is busy', () => {
    const markup = renderToStaticMarkup(
      <TopBar
        agentRunDisabled={true}
        busy={true}
        connection="live"
        exportDisabled={true}
        forceRerun={false}
        historyOpen={false}
        onAgentRun={() => {}}
        onExport={() => {}}
        onHistory={() => {}}
        onNewWorkspace={() => {}}
        onProviderSelect={() => {}}
        onForceRerunChange={() => {}}
        onQueue={() => {}}
        onUndo={() => {}}
        queueLockReason={{ kind: 'active_run' }}
        runDisabled={false}
        running={true}
        state={state}
        undoDisabled={true}
      />,
    );

    expect(markup).toContain('中断');
    expect(markup).toContain('title="中断当前运行"');
    expect(markup).not.toContain('title="中断当前运行" disabled=""');
  });

  it('renders unavailable provider status without Atlas fallback copy', () => {
    const markup = renderToStaticMarkup(
      <TopBar
        agentRunDisabled={true}
        busy={false}
        connection="live"
        exportDisabled={true}
        forceRerun={false}
        historyOpen={false}
        onAgentRun={() => {}}
        onExport={() => {}}
        onHistory={() => {}}
        onNewWorkspace={() => {}}
        onProviderSelect={() => {}}
        onForceRerunChange={() => {}}
        onQueue={() => {}}
        onUndo={() => {}}
        queueLockReason={{ kind: 'provider_unavailable', message: 'runtime provider `openai` is not configured by this build' }}
        runDisabled={true}
        running={false}
        state={{
          ...state,
          providers: {
            defaultProvider: 'openai',
            selectedProvider: 'openai',
            runtimeProviders: [
              {
                id: 'openai',
                label: 'openai',
                kind: 'unavailable',
                enabled: false,
                status: 'unavailable',
                message: 'runtime provider `openai` is not configured by this build',
                capabilities: [],
              },
            ],
            workflowBackends: [],
            apiConnectors: [],
          },
        }}
        undoDisabled={true}
      />,
    );

    expect(markup).toContain('openai');
    expect(markup).toContain('不可用');
    expect(markup).toContain('runtime provider `openai` is not configured by this build');
    expect(markup).not.toContain(['Atlas', '已配置'].join(' '));
    expect(markup).not.toContain(['Atlas', '未配置'].join(' '));
  });

  it('renders the selected provider and unavailable alternatives in the TopBar selector', () => {
    const markup = renderToStaticMarkup(
      <TopBar
        agentRunDisabled={false}
        busy={false}
        connection="live"
        exportDisabled={true}
        forceRerun={false}
        historyOpen={false}
        onAgentRun={() => {}}
        onExport={() => {}}
        onHistory={() => {}}
        onNewWorkspace={() => {}}
        onProviderSelect={() => {}}
        onForceRerunChange={() => {}}
        onQueue={() => {}}
        onUndo={() => {}}
        queueLockReason={{ kind: 'none' }}
        runDisabled={false}
        running={false}
        state={{
          ...state,
          providers: {
            ...state.providers,
            selectedProvider: 'atlas',
            runtimeProviders: [
              ...state.providers.runtimeProviders,
              {
                id: 'atlas',
                label: 'Atlas',
                kind: 'unavailable',
                enabled: false,
                status: 'unavailable',
                message: 'Atlas credentials are not configured',
                capabilities: ['image_generate'],
              },
            ],
          },
        }}
        undoDisabled={true}
      />,
    );

    expect(markup).toContain('aria-label="Runtime provider"');
    expect(markup).toContain('<option value="atlas" selected="">Atlas · 不可用</option>');
    expect(markup).toContain('Atlas credentials are not configured');
  });

  it('disables queue when provider-backed nodes use an unavailable provider', () => {
    const markup = renderToStaticMarkup(
      <App
        initialState={{
          ...state,
          pendingConfirmation: null,
          run: null,
          providers: {
            defaultProvider: 'openai',
            selectedProvider: 'openai',
            runtimeProviders: [
              {
                id: 'openai',
                label: 'openai',
                kind: 'unavailable',
                enabled: false,
                status: 'unavailable',
                message: 'runtime provider `openai` is not configured by this build',
                capabilities: [],
              },
            ],
            workflowBackends: [],
            apiConnectors: [],
          },
        }}
      />,
    );

    expect(markup).toContain('title="runtime provider `openai` is not configured by this build"');
    expect(markup).toContain('运行 Queue</button>');
    expect(markup).toContain('disabled=""');
  });

  it('renders agent runtime logs in a collapsed log group', () => {
    const markup = renderToStaticMarkup(
      <App
        initialState={{
          ...state,
          chat: {
            messages: [
              ...state.chat.messages,
              {
                id: 'agent-status-agent_session_1',
                role: 'agent',
                kind: 'agent_log:prompt',
                text: 'Prompt telemetry: mode=create_workflow',
                time: '09:11',
              },
            ],
          },
        }}
      />,
    );

    expect(markup).toContain('工具调用');
    expect(markup).toContain('1 events');
  });

  it('renders canvas ops evidence inside the tool log group', () => {
    const markup = renderToStaticMarkup(
      <App
        initialState={{
          ...state,
          chat: {
            messages: [
              ...state.chat.messages,
              {
                id: 'agent-canvas-ops-1',
                role: 'agent',
                kind: 'agent_log:canvas_ops',
                text: 'Canvas ops context ready: graph_nodes=2, selected_nodes=1',
                time: '09:11',
              },
            ],
          },
        }}
      />,
    );

    expect(markup).toContain('Canvas ops evidence');
    expect(markup).toContain('1 events');
  });

  it('applies websocket node state events to visible run state', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'ws_test',
      run_id: 'run_test_1',
      seq: 8,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'node.state',
      data: { node_id: 'video', state: 'running' },
    });

    expect(updated.eventSeq).toBe(8);
    expect(updated.graph.nodes.find((node) => node.id === 'video')?.status).toBe('running');
    expect(updated.run?.steps.find((step) => step.nodeId === 'video')?.state).toBe('running');
  });

  it('applies websocket failure events to visible diagnosis state', () => {
    const failedStep = applyRunEvent(state, {
      workspace_id: 'ws_test',
      run_id: 'run_test_1',
      seq: 8,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'node.state',
      data: { node_id: 'video', state: 'failed', error: 'provider rejected duration\nstack line 1' },
    });
    const failedRun = applyRunEvent(failedStep, {
      workspace_id: 'ws_test',
      run_id: 'run_test_1',
      seq: 9,
      server_time: '2026-06-12T00:00:02Z',
      ev: 'run.failed',
      data: { error: 'provider rejected duration\nstack line 1' },
    });

    expect(failedRun.graph.nodes.find((node) => node.id === 'video')?.status).toBe('failed');
    expect(failedRun.run?.status).toBe('failed');
    expect(failedRun.run?.error?.summary).toBe('provider rejected duration');
    expect(failedRun.run?.steps.find((step) => step.nodeId === 'video')?.error?.raw).toContain(
      'stack line',
    );
  });

  it('ignores websocket events for another run in the same workspace', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'ws_test',
      run_id: 'run_other',
      seq: 8,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'node.state',
      data: { node_id: 'video', state: 'running' },
    });

    expect(updated).toBe(state);
  });

  it('applies agent status events even when they use an agent session id', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'ws_test',
      run_id: 'agent_session_1',
      seq: 1,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'agent.status',
      data: {
        session_id: 'agent_session_1',
        status: 'runtime.status',
        detail: { message: 'Drafting graph proposal' },
      },
    });

    expect(updated.eventSeq).toBe(1);
    expect(updated.chat.messages.at(-1)).toMatchObject({
      id: 'agent-status-agent_session_1',
      role: 'agent',
      kind: 'agent_log:status',
      text: 'Drafting graph proposal',
    });
  });

  it('renders workspace state without a run record', () => {
    const markup = renderToStaticMarkup(<App initialState={{ ...state, run: null }} />);

    expect(markup).toContain('未运行');
  });

  it('renders failed run diagnosis card without raw error by default', () => {
    const markup = renderToStaticMarkup(<App initialState={failedRunState()} />);

    expect(markup).toContain('运行失败');
    expect(markup).toContain('provider rejected duration');
    expect(markup).toContain('查看 raw error');
    expect(markup).toContain('Create minimal fix proposal');
    expect(markup).toContain('node--err');
    expect(markup).not.toContain('stack line 1');
  });

  it('renders pending proposal actions from workspace state', () => {
    const markup = renderToStaticMarkup(
      <App
        initialState={{
          ...state,
          pendingProposal: pendingProposal(),
        }}
      />,
    );

    expect(markup).toContain('图变更提议');
    expect(markup).toContain('Shorter clip');
    expect(markup).toContain('忽略');
    expect(markup).toContain('应用到画布');
  });

  it('bootstraps by creating a blank workspace when none exists', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input);
        if (url === '/api/workspaces' && !init) {
          return jsonResponse([]);
        }
        if (url === '/api/workspaces' && init?.method === 'POST') {
          return jsonResponse({
            id: 'ws_new',
            name: 'Helixflow Workspace',
            versionId: 'ver_new',
            updatedAt: '2026-06-26T00:00:00Z',
          });
        }
        if (url === '/api/workspaces/ws_new/state') {
          return jsonResponse({
            ...state,
            workspace: {
              id: 'ws_new',
              name: 'Helixflow Workspace',
              versionId: 'ver_new',
              updatedAt: '2026-06-26T00:00:00Z',
            },
            run: null,
            outputs: [],
            pendingConfirmation: null,
            pendingProposal: null,
          });
        }
        return new Response('{}', { status: 404 });
      }),
    );

    await useWorkbenchStore.getState().bootstrap(null);

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock.mock.calls.map((call) => String(call[0]))).toEqual([
      '/api/workspaces',
      '/api/workspaces',
      '/api/workspaces/ws_new/state',
    ]);
    expect(useWorkbenchStore.getState().state?.workspace.id).toBe('ws_new');
  });

  it('posts composer messages to the workspace message API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return new Response(
          JSON.stringify({
            turnMode: 'chat',
            messages: [
              {
                id: 'msg_agent_1',
                role: 'agent',
                kind: 'chat',
                text: '我是 Helixflow agent。',
                time: 'unix:1',
              },
            ],
            proposal: null,
          }),
          {
            status: 200,
            headers: { 'content-type': 'application/json' },
          },
        );
      }),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().sendMessage('你好', {
      selection: { nodeIds: ['video'] },
    });

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/ws_test/messages', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: expect.any(String),
    });
    const body = JSON.parse(String(fetchMock.mock.calls[0][1]?.body));
    expect(body).toMatchObject({
      baseVersionId: 'ver_test_1',
      userMessage: '你好',
      canvasContext: { selection: { nodeIds: ['video'] } },
      graph: { schema_version: 1 },
    });
    expect(useWorkbenchStore.getState().state?.chat.messages.at(-1)).toMatchObject({
      role: 'agent',
      kind: 'chat',
      text: '我是 Helixflow agent。',
    });
  });

  it('rejects unknown canvas message context fields locally', () => {
    expect(
      CanvasMessageContextSchema.safeParse({
        selection: { nodeIds: ['video'], rawProviderConfig: true },
      }).success,
    ).toBe(false);
    expect(
      CanvasMessageContextSchema.safeParse({
        selection: { nodeIds: ['video'] },
        localPath: '/Users/example/private',
      }).success,
    ).toBe(false);
  });

  it('applies run request responses to pending confirmation state', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return new Response(
          JSON.stringify({
            turnMode: 'run_request',
            messages: [
              {
                id: 'msg_agent_run',
                role: 'agent',
                kind: 'run_requested',
                text: 'Run run_1 is waiting for confirmation.',
                time: 'unix:1',
              },
            ],
            proposal: null,
            run: {
              id: 'run_1',
              label: '运行当前 workflow',
              status: 'waiting_confirmation',
              steps: [
                { nodeId: 'text', title: 'text', state: 'queued', provider: null },
                { nodeId: 'video', title: 'video', state: 'queued', provider: 'mock' },
              ],
              cost: { estimate: 0, actual: 0, currency: 'USD' },
            },
            pendingConfirmation: {
              id: 'run_1',
              title: '运行当前 workflow',
              summary: 'Run is waiting for confirmation',
              cost: { amount: 0, currency: 'USD' },
            },
          }),
          {
            status: 200,
            headers: { 'content-type': 'application/json' },
          },
        );
      }),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().sendMessage('运行当前 workflow');

    const updated = useWorkbenchStore.getState().state;
    expect(updated?.run?.status).toBe('waiting_confirmation');
    expect(updated?.pendingConfirmation?.id).toBe('run_1');
    expect(updated?.chat.messages.at(-1)?.kind).toBe('run_requested');
  });

  it('queues the current workflow through the direct workbench run API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return jsonResponse({
          run: {
            id: 'run_direct_1',
            label: 'Manual workbench run',
            status: 'succeeded',
            steps: [
              { nodeId: 'text', title: 'text', state: 'succeeded', provider: null },
              { nodeId: 'video', title: 'video', state: 'succeeded', provider: 'mock' },
            ],
            cost: { estimate: 0, actual: 0, currency: 'USD' },
          },
          outputs: [
            {
              id: 'art_direct_1',
              kind: 'video',
              title: 'video',
              storageUri: 'workspace://outputs/run_direct_1/video.mp4',
              selected: true,
              meta: '{}',
            },
          ],
          pendingConfirmation: null,
        });
      }),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().queueRun();

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/ws_test/runs', {
      body: JSON.stringify({ forceRerun: false }),
      headers: { 'content-type': 'application/json' },
      method: 'POST',
    });
    expect(fetchMock.mock.calls[0][0]).not.toBe('/api/workspaces/ws_test/messages');
    const updated = useWorkbenchStore.getState().state;
    expect(updated?.run?.id).toBe('run_direct_1');
    expect(updated?.run?.status).toBe('succeeded');
    expect(updated?.outputs).toHaveLength(1);
    expect(updated?.pendingConfirmation).toBeNull();
  });

  it('passes force rerun through the direct workbench run API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return jsonResponse({
          run: {
            id: 'run_force_1',
            label: 'Manual workbench run',
            status: 'queued',
            steps: [],
            cost: { estimate: 0, actual: 0, currency: 'USD' },
          },
          outputs: [],
          pendingConfirmation: null,
        });
      }),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().queueRun({ forceRerun: true });

    expect(vi.mocked(fetch)).toHaveBeenCalledWith('/api/workspaces/ws_test/runs', {
      body: JSON.stringify({ forceRerun: true }),
      headers: { 'content-type': 'application/json' },
      method: 'POST',
    });
  });

  it('selects an output through the server-owned selection API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return jsonResponse({
          ...state,
          outputs: [
            {
              id: 'art_video_1',
              kind: 'video',
              title: 'Vertical teaser',
              storageUri: '/api/outputs/art_video_1/download',
              selected: false,
              meta: '{}',
              mime: 'video/mp4',
              preview: { kind: 'text', content: 'Artifact: Vertical teaser' },
            },
            {
              id: 'art_video_2',
              kind: 'video',
              title: 'Alternate teaser',
              storageUri: '/api/outputs/art_video_2/download',
              selected: true,
              meta: '{}',
              mime: 'video/mp4',
              preview: { kind: 'text', content: 'Artifact: Alternate teaser' },
            },
          ],
        });
      }),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().selectOutput('art_video_2');

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/outputs/art_video_2/select', {
      method: 'POST',
    });
    const updated = useWorkbenchStore.getState().state;
    expect(updated?.outputs.find((output) => output.selected)?.id).toBe('art_video_2');
    expect(updated?.outputs[1]?.preview?.content).toContain('Alternate teaser');
  });

  it('persists provider selection through the workspace provider API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          ...state,
          providers: {
            ...state.providers,
            selectedProvider: 'atlas',
            runtimeProviders: [
              ...state.providers.runtimeProviders,
              {
                id: 'atlas',
                label: 'Atlas',
                kind: 'external_api',
                enabled: true,
                status: 'healthy',
                message: null,
                capabilities: ['prompt_writer', 'image_generate', 'text_to_video'],
              },
            ],
          },
        }),
      ),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().selectProvider('atlas');

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/ws_test/provider', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ providerId: 'atlas' }),
    });
    expect(useWorkbenchStore.getState().state?.providers.selectedProvider).toBe('atlas');
  });

  it('renders the selected artifact preview instead of guessing from graph state', () => {
    const markup = renderToStaticMarkup(
      <App
        initialState={{
          ...state,
          outputs: [
            {
              id: 'art_video_1',
              kind: 'video',
              title: 'Vertical teaser',
              storageUri: '/api/outputs/art_video_1/download',
              selected: true,
              meta: '{}',
              mime: 'video/mp4',
              preview: { kind: 'text', content: 'Artifact: Vertical teaser' },
            },
          ],
        }}
      />,
    );

    expect(markup).toContain('artifact-stage');
    expect(markup).toContain('Artifact: Vertical teaser');
    expect(markup).not.toContain('canvas-grid');
  });

  it('keeps the graph canvas when the selected output has no preview', () => {
    const markup = renderToStaticMarkup(
      <App
        initialState={{
          ...state,
          outputs: [
            {
              id: 'art_selected_raw',
              kind: 'binary',
              title: 'Selected raw output',
              storageUri: '/api/outputs/art_selected_raw/download',
              selected: true,
              meta: '',
            },
            {
              id: 'art_preview_other',
              kind: 'video',
              title: 'Other preview',
              storageUri: '/api/outputs/art_preview_other/download',
              selected: false,
              meta: '{}',
              preview: { kind: 'text', content: 'Artifact: Other preview' },
            },
          ],
        }}
      />,
    );

    expect(markup).not.toContain('artifact-stage');
    expect(markup).not.toContain('Artifact: Other preview');
    expect(markup).toContain('canvas-grid');
  });

  it('renders graph canvas minimap when graph outputs have no preview', () => {
    const markup = renderToStaticMarkup(<App initialState={{ ...state, outputs: [] }} />);

    expect(markup).toContain('canvas-minimap');
    expect(markup).toContain('Graph minimap');
  });

  it('sandboxes HTML artifact previews without script permissions', () => {
    const markup = renderToStaticMarkup(
      <ArtifactStage
        outputs={[
          {
            id: 'art_html_1',
            kind: 'html',
            title: 'HTML report',
            storageUri: '/api/outputs/art_html_1/download',
            selected: true,
            meta: '{}',
            mime: 'text/html',
            preview: { kind: 'html', content: '<!doctype html><p>Report</p>' },
          },
        ]}
      />,
    );

    expect(markup).toContain('sandbox=""');
    expect(markup).not.toContain('allow-scripts');
  });

  it('renders image and video artifact previews from content URLs', () => {
    const imageMarkup = renderToStaticMarkup(
      <ArtifactStage
        outputs={[
          {
            id: 'art_image_1',
            kind: 'image',
            title: 'Atlas image',
            storageUri: '/api/artifacts/art_image_1/content',
            selected: true,
            meta: '{}',
            mime: 'image/png',
            preview: {
              kind: 'image',
              content: '/api/artifacts/art_image_1/content',
              mime: 'image/png',
            },
          },
        ]}
      />,
    );
    const videoMarkup = renderToStaticMarkup(
      <ArtifactStage
        outputs={[
          {
            id: 'art_video_1',
            kind: 'video',
            title: 'Atlas video',
            storageUri: '/api/artifacts/art_video_1/content',
            selected: true,
            meta: '{}',
            mime: 'video/mp4',
            preview: {
              kind: 'video',
              content: '/api/artifacts/art_video_1/content',
              mime: 'video/mp4',
            },
          },
        ]}
      />,
    );

    expect(imageMarkup).toContain('<img');
    expect(imageMarkup).toContain('src="/api/artifacts/art_image_1/content"');
    expect(videoMarkup).toContain('<video');
    expect(videoMarkup).toContain('controls=""');
    expect(videoMarkup).toContain('src="/api/artifacts/art_video_1/content"');
  });

  it('interrupts an active run through the direct interrupt API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return jsonResponse({
          run: {
            id: 'run_test_1',
            label: 'Manual preview',
            status: 'interrupted',
            steps: [
              { nodeId: 'text', title: 'text', state: 'succeeded', provider: null },
              { nodeId: 'video', title: 'video', state: 'skipped', provider: 'mock' },
            ],
            cost: { estimate: 0, actual: 0, currency: 'USD' },
          },
          outputs: [],
          pendingConfirmation: null,
        });
      }),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().interruptRun();

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/runs/run_test_1/interrupt', {
      method: 'POST',
    });
    const updated = useWorkbenchStore.getState().state;
    expect(updated?.run?.status).toBe('interrupted');
    expect(updated?.run?.steps.find((step) => step.nodeId === 'video')?.state).toBe('skipped');
  });

  it('exports the server-owned current workflow version instead of pending proposal preview', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return jsonResponse({
          schema_version: 1,
          nodes: {
            text: {
              node_type: 'input.text',
              title: 'Launch note',
              params: { text: 'Create a vertical product teaser.' },
              pos: [48, 158],
            },
            video: {
              node_type: 'video.text_to_video',
              title: 'Video render',
              params: {
                prompt: 'clean product shot',
                duration_sec: 5,
                aspect_ratio: '9:16',
              },
              pos: [486, 156],
            },
          },
          edges: [
            {
              from: ['text', 'text'],
              to: ['video', 'prompt'],
              edge_type: 'text',
            },
          ],
        });
      }),
    );
    useWorkbenchStore.getState().setInitialState({
      ...state,
      pendingProposal: pendingProposal(),
    });

    const exported = await useWorkbenchStore.getState().exportWorkflow();

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/versions/ver_test_1/export');
    expect(exported?.nodes.video.params).toMatchObject({ duration_sec: 5 });
    expect(useWorkbenchStore.getState().state?.pendingProposal?.id).toBe('proposal_1');
  });

  it('undoes the current workflow through the workspace version API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return jsonResponse(restoredState('ver_undo_1', 5));
      }),
    );
    useWorkbenchStore.getState().setInitialState({
      ...state,
      history: [
        { id: 'ver_base', kind: 'version', label: 'Base graph', time: '09:01', summary: 'manual graph' },
        { id: 'ver_test_1', kind: 'version', label: 'Shorter clip', time: '09:05', summary: 'proposal graph' },
      ],
    });

    await useWorkbenchStore.getState().undoVersion();

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/ws_test/versions/undo', {
      method: 'POST',
    });
    const updated = useWorkbenchStore.getState().state;
    expect(updated?.workspace.versionId).toBe('ver_undo_1');
    expect(updated?.workflowGraph?.nodes.video.params).toMatchObject({ duration_sec: 5 });
  });

  it('restores a history version through the workspace version API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return jsonResponse(restoredState('ver_restore_1', 5));
      }),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().restoreVersion('ver_base');

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/ws_test/versions/ver_base/restore', {
      method: 'POST',
    });
    expect(useWorkbenchStore.getState().state?.workspace.versionId).toBe('ver_restore_1');
  });

  it('saves graph layout through the workspace version API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          ...state,
          workspace: { ...state.workspace, versionId: 'ver_layout_1' },
          graph: {
            ...state.graph,
            nodes: state.graph.nodes.map((node) =>
              node.id === 'video' ? { ...node, position: { x: 620, y: 210 } } : node,
            ),
          },
        }),
      ),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().saveLayout([{ id: 'video', x: 620, y: 210 }]);

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/ws_test/versions/layout', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: expect.any(String),
    });
    expect(JSON.parse(String(fetchMock.mock.calls[0][1]?.body))).toEqual({
      baseVersionId: 'ver_test_1',
      positions: [{ id: 'video', x: 620, y: 210 }],
    });
    expect(useWorkbenchStore.getState().state?.workspace.versionId).toBe('ver_layout_1');
    expect(useWorkbenchStore.getState().state?.graph.nodes.find((node) => node.id === 'video')?.position).toEqual({
      x: 620,
      y: 210,
    });
  });

  it('renders restore actions for historical version rows', () => {
    const markup = renderToStaticMarkup(
      <HistoryPanel
        busy={false}
        currentVersionId="ver_current"
        currentWorkspaceId="ws_test"
        history={[
          { id: 'ver_base', kind: 'version', label: 'Base graph', time: '09:01', summary: 'manual graph' },
          { id: 'ver_current', kind: 'version', label: 'Current graph', time: '09:05', summary: 'proposal graph' },
        ]}
        onClose={() => undefined}
        onOpenWorkspace={() => undefined}
        onRestoreVersion={() => undefined}
        open
        workspaceListError={null}
        workspaces={[]}
      />,
    );

    expect(markup).toContain('恢复');
    expect(markup).toContain('当前');
  });

  it('resets per-run event sequencing when a new run snapshot arrives', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return jsonResponse({
          turnMode: 'run_request',
          messages: [],
          proposal: null,
          run: {
            id: 'run_new',
            label: '运行当前 workflow',
            status: 'running',
            steps: [
              { nodeId: 'text', title: 'text', state: 'queued', provider: null },
              { nodeId: 'video', title: 'video', state: 'queued', provider: 'mock' },
            ],
            cost: { estimate: 0, actual: 0, currency: 'USD' },
          },
          pendingConfirmation: null,
        });
      }),
    );
    useWorkbenchStore.getState().setInitialState({ ...state, eventSeq: 8 });

    await useWorkbenchStore.getState().sendMessage('运行当前 workflow');
    const nextRun = useWorkbenchStore.getState().state;

    expect(nextRun?.eventSeq).toBe(0);
    expect(nextRun?.run?.id).toBe('run_new');

    useWorkbenchStore.getState().applyEvent({
      workspace_id: 'ws_test',
      run_id: 'run_new',
      seq: 1,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'node.state',
      data: { node_id: 'video', state: 'running' },
    });

    const updated = useWorkbenchStore.getState().state;
    expect(updated?.eventSeq).toBe(1);
    expect(updated?.run?.steps.find((step) => step.nodeId === 'video')?.state).toBe('running');
  });

  it('applies proposal responses to pending proposal state', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return jsonResponse({
          turnMode: 'create_workflow',
          messages: [
            {
              id: 'msg_agent_proposal',
              role: 'agent',
              kind: 'proposal_pending',
              text: 'Set duration to four seconds.',
              time: 'unix:1',
            },
          ],
          proposal: pendingProposal(),
        });
      }),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().sendMessage('创建一个 workflow');

    const updated = useWorkbenchStore.getState().state;
    expect(updated?.pendingProposal?.id).toBe('proposal_1');
    expect(updated?.chat.messages.at(-1)?.kind).toBe('proposal_pending');
  });

  it('confirms a pending run through the run confirmation API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return new Response(
          JSON.stringify({
            run: {
              id: 'run_test_1',
              label: 'Manual preview',
              status: 'succeeded',
              steps: [
                { nodeId: 'text', title: 'text', state: 'succeeded', provider: null },
                { nodeId: 'video', title: 'video', state: 'succeeded', provider: 'mock' },
              ],
              cost: { estimate: 0, actual: 0, currency: 'USD' },
            },
            outputs: [],
            pendingConfirmation: null,
          }),
          {
            status: 200,
            headers: { 'content-type': 'application/json' },
          },
        );
      }),
    );
    useWorkbenchStore.getState().setInitialState({
      ...state,
      pendingConfirmation: {
        id: 'run_test_1',
        title: 'Manual preview',
        summary: 'Run is waiting for confirmation',
        cost: { amount: 0, currency: 'USD' },
      },
    });

    await useWorkbenchStore.getState().confirmRun('run_test_1');

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/ws_test/runs/run_test_1/confirm', {
      method: 'POST',
    });
    const updated = useWorkbenchStore.getState().state;
    expect(updated?.run?.status).toBe('succeeded');
    expect(updated?.pendingConfirmation).toBeNull();
    expect(updated?.graph.nodes.find((node) => node.id === 'video')?.status).toBe('succeeded');
  });

  it('marks a confirming run as running before the confirmation response returns', async () => {
    const fetchControl: { resolve?: (response: Response) => void } = {};
    vi.stubGlobal(
      'fetch',
      vi.fn(
        () =>
          new Promise<Response>((resolve) => {
            fetchControl.resolve = resolve;
          }),
      ),
    );
    useWorkbenchStore.getState().setInitialState({
      ...state,
      run: {
        ...(state.run as NonNullable<WorkbenchState['run']>),
        status: 'waiting_confirmation',
      },
      pendingConfirmation: {
        id: 'run_test_1',
        title: 'Manual preview',
        summary: 'Run is waiting for confirmation',
        cost: { amount: 0, currency: 'USD' },
      },
    });

    const pending = useWorkbenchStore.getState().confirmRun('run_test_1');

    expect(useWorkbenchStore.getState().state?.run?.status).toBe('running');
    expect(useWorkbenchStore.getState().state?.pendingConfirmation).toBeNull();
    if (!fetchControl.resolve) {
      throw new Error('fetch resolver was not installed');
    }
    fetchControl.resolve(
      jsonResponse({
        run: {
          id: 'run_test_1',
          label: 'Manual preview',
          status: 'interrupted',
          steps: [
            { nodeId: 'text', title: 'text', state: 'skipped', provider: null },
            { nodeId: 'video', title: 'video', state: 'skipped', provider: 'mock' },
          ],
          cost: { estimate: 0, actual: 0, currency: 'USD' },
        },
        outputs: [],
        pendingConfirmation: null,
      }),
    );
    await pending;

    expect(useWorkbenchStore.getState().state?.run?.status).toBe('interrupted');
  });

  it('restores pending confirmation when a confirm request fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse(
          {
            error: 'provider unavailable',
          },
          503,
        ),
      ),
    );
    useWorkbenchStore.getState().setInitialState({
      ...state,
      run: {
        ...(state.run as NonNullable<WorkbenchState['run']>),
        status: 'waiting_confirmation',
      },
      pendingConfirmation: {
        id: 'run_test_1',
        title: 'Manual preview',
        summary: 'Run is waiting for confirmation',
        cost: { amount: 0, currency: 'USD' },
      },
    });

    await useWorkbenchStore.getState().confirmRun('run_test_1');

    const updated = useWorkbenchStore.getState().state;
    expect(updated?.run?.status).toBe('waiting_confirmation');
    expect(updated?.pendingConfirmation?.id).toBe('run_test_1');
    expect(updated?.chat.messages.at(-1)).toMatchObject({
      role: 'system',
      kind: 'run_failed',
      text: 'provider unavailable',
    });
  });

  it('holds a pending run through the run hold API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        return new Response(
          JSON.stringify({
            run: {
              id: 'run_test_1',
              label: 'Manual preview',
              status: 'interrupted',
              steps: [
                { nodeId: 'text', title: 'text', state: 'queued', provider: null },
                { nodeId: 'video', title: 'video', state: 'queued', provider: 'mock' },
              ],
              cost: { estimate: 0, actual: 0, currency: 'USD' },
            },
            outputs: [],
            pendingConfirmation: null,
          }),
          {
            status: 200,
            headers: { 'content-type': 'application/json' },
          },
        );
      }),
    );
    useWorkbenchStore.getState().setInitialState({
      ...state,
      pendingConfirmation: {
        id: 'run_test_1',
        title: 'Manual preview',
        summary: 'Run is waiting for confirmation',
        cost: { amount: 0, currency: 'USD' },
      },
    });

    await useWorkbenchStore.getState().holdRun('run_test_1');

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/ws_test/runs/run_test_1/hold', {
      method: 'POST',
    });
    const updated = useWorkbenchStore.getState().state;
    expect(updated?.run?.status).toBe('interrupted');
    expect(updated?.pendingConfirmation).toBeNull();
  });

  it('applies and dismisses proposals through proposal APIs', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input);
        if (url.endsWith('/apply')) {
          return jsonResponse({
            ...state,
            workspace: { ...state.workspace, versionId: 'ver_applied' },
            pendingProposal: null,
          });
        }
        if (url.endsWith('/dismiss')) {
          return jsonResponse({
            ...state,
            pendingProposal: null,
          });
        }
        return new Response('{}', { status: 404 });
      }),
    );
    useWorkbenchStore.getState().setInitialState({
      ...state,
      pendingProposal: pendingProposal(),
    });

    await useWorkbenchStore.getState().applyProposal('proposal_1');
    await useWorkbenchStore.getState().dismissProposal('proposal_1');

    const fetchMock = vi.mocked(fetch);
    expect(fetchMock.mock.calls[0]).toEqual([
      '/api/workspaces/ws_test/proposals/proposal_1/apply',
      { method: 'POST' },
    ]);
    expect(fetchMock.mock.calls[1]).toEqual([
      '/api/workspaces/ws_test/proposals/proposal_1/dismiss',
      { method: 'POST' },
    ]);
  });

  it('applies manual edits through the bounded manual ops API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          ...state,
          pendingProposal: pendingProposal(),
        }),
      ),
    );
    useWorkbenchStore.getState().setInitialState(state);

    await useWorkbenchStore.getState().createManualProposal({
      baseVersionId: 'ver_test_1',
      ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', value: 4 }],
    });

    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0]!;
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/workspaces/ws_test/versions/ops');
    expect(init).toMatchObject({
      method: 'POST',
      headers: { 'content-type': 'application/json' },
    });
    expect(JSON.parse(String((init as RequestInit).body))).toEqual({
      baseVersionId: 'ver_test_1',
      ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', value: 4 }],
    });
    expect(useWorkbenchStore.getState().state?.pendingProposal?.id).toBe('proposal_1');
  });

  it('surfaces manual edit API errors in state and to the caller', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: 'invalid param' }, 400)));
    useWorkbenchStore.getState().setInitialState(state);

    await expect(
      useWorkbenchStore.getState().createManualProposal({
        baseVersionId: 'ver_test_1',
        ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', value: 'slow' }],
      }),
    ).rejects.toThrow('invalid param');

    expect(useWorkbenchStore.getState().state?.chat.messages.at(-1)?.text).toBe('invalid param');
  });

});

describe('GraphCanvas navigation', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('loads and saves viewport per workspace', () => {
    const store = new Map<string, string>();
    vi.stubGlobal('localStorage', localStorageStub(store));

    saveGraphCanvasView('ws-a', { x: 120, y: -40, z: 1.2 });

    expect(store.has(viewStorageKey('ws-a'))).toBe(true);
    expect(loadGraphCanvasView('ws-a')).toEqual({ x: 120, y: -40, z: 1.2 });
    expect(loadGraphCanvasView('ws-b')).toEqual(DEFAULT_GRAPH_VIEW);

    store.set(viewStorageKey('ws-a'), '{"x":10,"y":20,"z":99}');
    expect(loadGraphCanvasView('ws-a')).toEqual({ x: 10, y: 20, z: 1.4 });

    store.set(viewStorageKey('ws-a'), 'not-json');
    expect(loadGraphCanvasView('ws-a')).toEqual(DEFAULT_GRAPH_VIEW);
  });

  it('keeps the pointer world coordinate stable during wheel zoom', () => {
    const before = { x: 20, y: 18, z: 0.78 };
    const localX = 320;
    const localY = 180;
    const worldBefore = {
      x: (localX - before.x) / before.z,
      y: (localY - before.y) / before.z,
    };

    const after = zoomViewAtPoint(before, { deltaY: -160, localX, localY });

    expect(after.z).toBeGreaterThan(before.z);
    expect((localX - after.x) / after.z).toBeCloseTo(worldBefore.x);
    expect((localY - after.y) / after.z).toBeCloseTo(worldBefore.y);
  });

  it('computes minimap navigation from graph bounds without mutating graph data', () => {
    const layout = computeMinimapLayout(state.graph.nodes, { width: 188, height: 124 });

    expect(layout?.nodes).toHaveLength(2);
    expect(layout?.nodes[0]?.width).toBeGreaterThan(0);

    const current = { x: 20, y: 18, z: 0.78 };
    const next = viewForMinimapPoint(layout!, { x: 94, y: 62 }, current, {
      width: 900,
      height: 640,
    });
    const viewport = minimapViewportRect(layout!, next, { width: 900, height: 640 });

    expect(next.z).toBe(current.z);
    expect(next.x).not.toBe(current.x);
    expect(viewport.width).toBeGreaterThan(0);
  });

  it('keeps pending proposal preview graph and diff styling while navigation UI is present', () => {
    const markup = renderToStaticMarkup(
      <GraphCanvas
        graph={state.graph}
        pendingProposal={pendingProposal()}
        run={state.run!}
        versionId="ver_test_1"
        workspaceId="ws_test"
      />,
    );

    expect(markup).toContain('待确认的图变更');
    expect(markup).toContain('node--upd');
    expect(markup).toContain('node--locked');
    expect(markup).toContain('canvas-minimap');
    expect(markup).not.toContain('保存布局');
  });
});

describe('GraphCanvas layout editing', () => {
  it('computes single-node and multi-node layout drafts without mutating base nodes', () => {
    const drafts = moveNodeDrafts(
      [
        { id: 'text', x: 48, y: 158 },
        { id: 'video', x: 486, y: 156 },
      ],
      { x: 24, y: -18 },
    );

    const moved = applyPositionDrafts(state.graph.nodes, drafts);
    const updates = positionUpdatesFromDrafts(state.graph.nodes, drafts);

    expect(moved.find((node) => node.id === 'text')?.position).toEqual({ x: 72, y: 140 });
    expect(moved.find((node) => node.id === 'video')?.position).toEqual({ x: 510, y: 138 });
    expect(state.graph.nodes.find((node) => node.id === 'text')?.position).toEqual({
      x: 48,
      y: 158,
    });
    expect(updates).toEqual([
      { id: 'text', x: 72, y: 140 },
      { id: 'video', x: 510, y: 138 },
    ]);
  });

  it('preserves selected groups for drag and toggles modifier selection', () => {
    const group = new Set(['text', 'video']);

    expect([...selectionForNodePointer(group, 'video', false)]).toEqual(['text', 'video']);
    expect([...selectionForNodePointer(group, 'video', true)]).toEqual(['text']);
    expect([...selectionForNodePointer(new Set(['text']), 'video', true)]).toEqual([
      'text',
      'video',
    ]);
  });
});

describe('GraphCanvas rendering helpers', () => {
  it('builds stable maps and sets for large graph rendering lookups', () => {
    const nodes = Array.from({ length: 120 }, (_, index) => ({
      ...state.graph.nodes[index % state.graph.nodes.length],
      id: `node_${index}`,
      position: { x: index * 12, y: index * 5 },
    }));
    const steps = nodes.map((node, index) => ({
      nodeId: node.id,
      title: node.title,
      state: index === 80 ? 'running' as const : 'queued' as const,
      provider: node.provider,
    }));
    const edges = [
      {
        id: 'edge_large_1',
        from: { nodeId: 'node_1', port: 'text' },
        to: { nodeId: 'node_2', port: 'prompt' },
        kind: 'text',
      },
    ];

    const nodeById = buildNodeMap(nodes);
    const stepStateByNodeId = buildRunStepStateMap(steps);
    const edgeIds = buildEdgeSignatureSet(edges);

    expect(nodeById.get('node_80')?.position).toEqual({ x: 960, y: 400 });
    expect(stepStateByNodeId.get('node_80')).toBe('running');
    expect(edgeIds.has('node_1:text>node_2:prompt:text')).toBe(true);
  });

  it('detects proposal node additions and updates from comparable node snapshots', () => {
    const baseNode = state.graph.nodes[0];
    const baseComparable = buildComparableNodeMap([baseNode]);

    expect(nodeDiffState(baseNode, baseComparable.get(baseNode.id), true)).toBeNull();
    expect(
      nodeDiffState({ ...baseNode, title: 'Updated title' }, baseComparable.get(baseNode.id), true),
    ).toBe('upd');
    expect(nodeDiffState({ ...baseNode, id: 'new_node' }, undefined, true)).toBe('add');
    expect(nodeDiffState({ ...baseNode, title: 'Updated title' }, baseComparable.get(baseNode.id), false)).toBeNull();
  });
});

describe('GraphCanvas selection and clipboard helpers', () => {
  it('renders a multi-selection inspector summary', () => {
    const markup = renderToStaticMarkup(
      <GraphSelectionInspector nodes={state.graph.nodes} onClose={() => undefined} />,
    );

    expect(markup).toContain('多选 · 2 个节点');
    expect(markup).toContain('Selection summary');
    expect(markup).toContain('Launch note');
  });

  it('selects nodes from local drag rectangles in graph world coordinates', () => {
    const localRect = selectionRectFromPoints({ x: 50, y: 40 }, { x: 380, y: 260 });
    const worldRect = worldRectFromLocalRect(localRect, { x: 20, y: 18, z: 0.78 });
    const hitIds = selectedIdsInWorldRect(state.graph.nodes, worldRect);

    expect(localRect).toEqual({ x: 50, y: 40, width: 330, height: 220 });
    expect(hitIds.has('text')).toBe(true);
    expect(hitIds.has('video')).toBe(false);
    expect([...mergeSelection(new Set(['video']), hitIds, true)].sort()).toEqual(['text', 'video']);
  });

  it('maps canvas keyboard shortcuts while skipping IME composition', () => {
    expect(graphShortcutFromEvent({ key: 'Escape' })).toBe('clear_selection');
    expect(graphShortcutFromEvent({ key: 'a', metaKey: true })).toBe('select_all');
    expect(graphShortcutFromEvent({ key: '0', ctrlKey: true })).toBe('fit_view');
    expect(graphShortcutFromEvent({ key: 'c', ctrlKey: true })).toBe('copy_selection');
    expect(graphShortcutFromEvent({ key: 'c' })).toBeNull();
    expect(graphShortcutFromEvent({ key: 'c', ctrlKey: true, nativeEvent: { isComposing: true } })).toBeNull();
    expect(graphShortcutFromEvent({ key: 'a', metaKey: true, nativeEvent: { keyCode: 229 } })).toBeNull();
  });

  it('fits view to visible nodes without mutating graph data', () => {
    const next = fitViewToNodes(state.graph.nodes, { width: 900, height: 640 });

    expect(next.z).toBeGreaterThan(0.4);
    expect(next.z).toBeLessThanOrEqual(1.4);
    expect(next.x).not.toBe(DEFAULT_GRAPH_VIEW.x);
  });

  it('copies stable sanitized selection text without provider or local path fields', () => {
    const nodes = [
      {
        ...state.graph.nodes[0],
        title: 'Input sk-secretvalue123456',
        summary: 'Read /Users/alice/private.txt with apiKey and https://example.test/raw',
        provider: 'mock-secret-provider',
      },
      state.graph.nodes[1],
    ];
    const text = selectionClipboardText(nodes, state.graph.edges);

    expect(text).toContain('Helixflow selection (2 nodes)');
    expect(text).toContain('"schema_version": 1');
    expect(text).toContain('"edge_type": "text"');
    expect(text).not.toContain('mock-secret-provider');
    expect(text).not.toContain('/Users/alice');
    expect(text).not.toContain('sk-secretvalue123456');
    expect(text).not.toContain('https://example.test');
    expect(sanitizeClipboardText('C:\\Users\\alice\\token.txt')).toContain('[redacted-path]');
  });
});

describe('ManualProposalPanel', () => {
  it('builds bounded manual edit inputs and validates JSON locally', () => {
    const input = buildManualProposalInput({
      baseVersionId: 'ver_test_1',
      edgeIndex: '0',
      edgeType: 'text',
      fromNode: 'text',
      fromPort: 'text',
      nodeId: 'manual_text',
      nodeTitle: '',
      nodeType: 'input.text',
      operation: 'set_param',
      paramKey: 'duration_sec',
      paramValue: '4',
      paramsText: '{"text":"manual input"}',
      targetNodeId: 'video',
      toNode: 'video',
      toPort: 'prompt',
      workflowGraph: state.workflowGraph,
      x: '120',
      y: '320',
    });

    expect(input).toEqual({
      baseVersionId: 'ver_test_1',
      label: 'Manual set param',
      ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', value: 4 }],
    });
    expect(() => parseManualJson('{bad')).toThrow('Invalid JSON');
  });

  it('renders manual controls and pending disabled state', () => {
    const markup = renderToStaticMarkup(
      <ManualProposalPanel
        busy={false}
        initialCatalog={nodeCatalog()}
        state={{ ...state, pendingProposal: pendingProposal() }}
        onCreateProposal={async () => undefined}
      />,
    );

    expect(markup).toContain('Advanced edit');
    expect(markup).toContain('agent pending');
    expect(markup).toContain('edit param');
    expect(markup).toContain('text · Launch note');
    expect(markup).toContain('Add to edit session');
  });

  it('derives valid default params from required catalog schema values', () => {
    const definition = nodeCatalog().nodes.find((item) => item.type === 'video.text_to_video')!;

    expect(defaultParamsForDefinition(definition)).toEqual({
      prompt: '',
      duration_sec: 1,
      aspect_ratio: '1:1',
    });
  });
});

describe('chat composer keyboard handling', () => {
  it('does not submit Enter while IME composition is active', () => {
    expect(shouldSubmitComposerKey({
      key: 'Enter',
      shiftKey: false,
      nativeEvent: { isComposing: true },
    })).toBe(false);

    expect(shouldSubmitComposerKey({
      key: 'Enter',
      shiftKey: false,
      nativeEvent: { keyCode: 229 },
    })).toBe(false);

    expect(shouldSubmitComposerKey({
      key: 'Enter',
      shiftKey: false,
      nativeEvent: {},
    }, true)).toBe(false);
  });

  it('submits regular Enter but keeps Shift+Enter for new lines', () => {
    expect(shouldSubmitComposerKey({
      key: 'Enter',
      shiftKey: false,
      nativeEvent: {},
    })).toBe(true);

    expect(shouldSubmitComposerKey({
      key: 'Enter',
      shiftKey: true,
      nativeEvent: {},
    })).toBe(false);
  });
});

function jsonResponse(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

function localStorageStub(store: Map<string, string>): Storage {
  return {
    get length() {
      return store.size;
    },
    clear: () => store.clear(),
    getItem: (key: string) => store.get(key) ?? null,
    key: (index: number) => Array.from(store.keys())[index] ?? null,
    removeItem: (key: string) => {
      store.delete(key);
    },
    setItem: (key: string, value: string) => {
      store.set(key, value);
    },
  };
}

function nodeCatalog(): NodeCatalog {
  return {
    schema_version: 1,
    nodes: [
      {
        type: 'input.text',
        title: 'Text Input',
        category: 'input',
        provider: null,
        capability: null,
        description: 'A user-provided text value.',
        inputs: [],
        outputs: [{ name: 'text', type: 'TEXT' as const, required: true }],
        params_schema: {
          required: ['text'],
          properties: {
            text: { type: 'string' as const, enum_values: [], minimum: null, maximum: null },
          },
          allow_unknown: false,
        },
        estimated_cost: null,
      },
      {
        type: 'video.text_to_video',
        title: 'Text To Video',
        category: 'video',
        provider: 'mock',
        capability: 'text_to_video',
        description: 'Generates a deterministic placeholder video artifact.',
        inputs: [{ name: 'prompt', type: 'TEXT' as const, required: true }],
        outputs: [{ name: 'video', type: 'VIDEO' as const, required: true }],
        params_schema: {
          required: ['prompt', 'duration_sec', 'aspect_ratio'],
          properties: {
            prompt: { type: 'string' as const, enum_values: [], minimum: null, maximum: null },
            duration_sec: { type: 'integer' as const, enum_values: [], minimum: 1, maximum: 10 },
            aspect_ratio: {
              type: 'string' as const,
              enum_values: ['1:1', '9:16', '16:9'],
              minimum: null,
              maximum: null,
            },
          },
          allow_unknown: false,
        },
        estimated_cost: { unit: 'call', catalog_key: 'mock.text_to_video' },
      },
    ],
  };
}

function pendingProposal(): NonNullable<WorkbenchState['pendingProposal']> {
  return {
    id: 'proposal_1',
    baseVersionId: 'ver_test_1',
    kind: 'modify',
    title: 'Shorter clip',
    summary: 'Set duration to four seconds.',
    ops: [],
    diffSummary: [],
    previewGraph: {
      nodes: [
        {
          id: 'video',
          nodeType: 'video.text_to_video',
          title: 'Video render',
          category: 'Video',
          status: 'queued',
          position: { x: 486, y: 156 },
          provider: 'mock',
          summary: JSON.stringify({
            prompt: 'clean product shot',
            duration_sec: 4,
            aspect_ratio: '9:16',
          }),
        },
      ],
      edges: [],
    },
    state: 'pending',
    messageId: 'msg_agent_proposal',
  };
}

function restoredState(versionId: string, durationSec: number): WorkbenchState {
  return {
    ...state,
    workspace: {
      ...state.workspace,
      versionId,
    },
    graph: {
      ...state.graph,
      nodes: state.graph.nodes.map((node) =>
        node.id === 'video' ? { ...node, summary: `${durationSec} seconds` } : node,
      ),
    },
    workflowGraph: {
      schema_version: 1,
      nodes: {
        text: {
          node_type: 'input.text',
          title: 'Launch note',
          params: { text: 'Create a vertical product teaser.' },
          pos: [48, 158],
        },
        video: {
          node_type: 'video.text_to_video',
          title: 'Video render',
          params: {
            prompt: 'clean product shot',
            duration_sec: durationSec,
            aspect_ratio: '9:16',
          },
          pos: [486, 156],
        },
      },
      edges: [
        {
          from: ['text', 'text'],
          to: ['video', 'prompt'],
          edge_type: 'text',
        },
      ],
    },
    history: [
      ...state.history,
      {
        id: versionId,
        kind: 'version',
        label: 'Restore Base graph',
        time: '09:06',
        summary: 'restore graph',
      },
    ],
  };
}

function failedRunState(): WorkbenchState {
  return {
    ...state,
    graph: {
      ...state.graph,
      nodes: state.graph.nodes.map((node) =>
        node.id === 'video' ? { ...node, status: 'failed' } : node,
      ),
    },
    run: {
      ...state.run!,
      status: 'failed',
      error: {
        summary: 'provider rejected duration',
        raw: '{"error":"provider rejected duration","trace":"stack line 1"}',
      },
      steps: state.run!.steps.map((step) =>
        step.nodeId === 'video'
          ? {
              ...step,
              state: 'failed',
              error: {
                summary: 'provider rejected duration',
                raw: '{"error":"provider rejected duration","trace":"stack line 1"}',
              },
            }
          : step,
      ),
    },
  };
}
