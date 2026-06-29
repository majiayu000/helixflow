import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './app';
import { ArtifactStage } from './components/artifact-stage';
import { shouldSubmitComposerKey } from './components/chat-pane';
import { ConfirmModal, HistoryPanel } from './components/run-panels';
import { TopBar } from './components/top-bar';
import { applyRunEvent, useWorkbenchStore } from './store';
import type { WorkbenchState } from './types';

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
    providers: [
      {
        id: 'mock',
        displayName: 'Mock',
        configured: true,
        enabled: true,
        label: 'Mock runtime',
        endpoint: null,
        health: { ok: true, message: 'Mock runtime provider configured' },
      },
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
        nodeType: 'video.mock.text_to_video',
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
};

describe('App', () => {
  beforeEach(() => {
    useWorkbenchStore.setState({
      status: 'idle',
      error: null,
      connection: 'offline',
      state: null,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('renders the primary workbench layout from backend state', () => {
    const markup = renderToStaticMarkup(<App initialState={state} />);

    expect(markup).toContain('ComfyUI Agent');
    expect(markup).toContain('Test Workspace');
    expect(markup).toContain('对话');
    expect(markup).toContain('2 节点');
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
        historyOpen={false}
        onAgentRun={() => {}}
        onExport={() => {}}
        onHistory={() => {}}
        onNewWorkspace={() => {}}
        onQueue={() => {}}
        onUndo={() => {}}
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

    await useWorkbenchStore.getState().sendMessage('你好');

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
      graph: { schema_version: 1 },
    });
    expect(useWorkbenchStore.getState().state?.chat.messages.at(-1)).toMatchObject({
      role: 'agent',
      kind: 'chat',
      text: '我是 Helixflow agent。',
    });
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
      method: 'POST',
    });
    expect(fetchMock.mock.calls[0][0]).not.toBe('/api/workspaces/ws_test/messages');
    const updated = useWorkbenchStore.getState().state;
    expect(updated?.run?.id).toBe('run_direct_1');
    expect(updated?.run?.status).toBe('succeeded');
    expect(updated?.outputs).toHaveLength(1);
    expect(updated?.pendingConfirmation).toBeNull();
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
              node_type: 'video.mock.text_to_video',
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

function jsonResponse(value: unknown): Response {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
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
          nodeType: 'video.mock.text_to_video',
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
          node_type: 'video.mock.text_to_video',
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
