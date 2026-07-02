import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fetchWorkspaceList, fetchWorkspaceState } from './api';
import { App } from './app';
import { HistoryPanel } from './components/run-panels';
import { applyRunEvent, useWorkbenchStore } from './store';
import type { WorkbenchState } from './types';

const state: WorkbenchState = {
  eventSeq: 0,
  workspace: {
    id: 'demo',
    name: 'Helixflow Demo',
    versionId: 'ver_demo_1',
    updatedAt: '2026-06-12T00:00:00Z',
  },
  providers: {
    defaultProvider: 'atlas',
    providers: [
      {
        id: 'atlas',
        displayName: 'Atlas',
        configured: true,
        enabled: true,
        label: 'Atlas API',
        endpoint: 'https://api.atlascloud.ai/v1',
        health: { ok: true, message: 'Atlas API provider configured' },
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
        id: 'save',
        nodeType: 'output.save',
        title: 'Save output',
        category: 'Output',
        status: 'queued',
        position: { x: 486, y: 156 },
        provider: null,
        summary: '{}',
      },
    ],
    edges: [
      {
        id: 'edge_text_save',
        from: { nodeId: 'text', port: 'text' },
        to: { nodeId: 'save', port: 'artifact' },
        kind: 'artifact',
      },
    ],
  },
  run: {
    id: 'run_demo_1',
    label: 'Manual preview',
    status: 'running',
    steps: [
      { nodeId: 'text', title: 'Launch note', state: 'succeeded', provider: null },
      { nodeId: 'save', title: 'Save output', state: 'queued', provider: null },
    ],
    cost: { estimate: 0, actual: 0, currency: 'USD' },
  },
  outputs: [
    {
      id: 'art_text_1',
      kind: 'text',
      title: 'Saved text',
      storageUri: 'workspace://inputs/run_demo_1/text/text.txt',
      selected: true,
      meta: 'text/plain',
    },
  ],
  history: [
    {
      id: 'hist_1',
      kind: 'version',
      label: 'Initial graph',
      time: '09:05',
      summary: 'Version ver_demo_1',
    },
  ],
  pendingProposal: null,
  pendingConfirmation: {
    id: 'confirm_1',
    title: 'Agent requested run',
    summary: 'Run the current graph.',
    cost: { amount: 0, currency: 'USD' },
  },
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
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('renders the primary workbench layout from backend state', () => {
    const markup = renderToStaticMarkup(<App initialState={state} />);

    expect(markup).toContain('ComfyUI Agent');
    expect(markup).toContain('Helixflow Demo');
    expect(markup).toContain('对话');
    expect(markup).toContain('2 节点');
    expect(markup).toContain('运行中');
    expect(markup).toContain('1 个真实 artifact');
    expect(markup).toContain('版本与运行历史');
    expect(markup).toContain('Agent requested run');
  });

  it('keeps the workflow canvas primary when previewable artifacts exist', () => {
    const artifactState: WorkbenchState = {
      ...state,
      graph: { nodes: [], edges: [] },
      outputs: [
        {
          id: 'art_html_1',
          kind: 'html',
          title: 'Prototype',
          storageUri: 'workspace://workspaces/demo/artifacts/agent_1/index.html',
          selected: true,
          meta: '{}',
          mime: 'text/html',
          preview: {
            kind: 'html',
            content: '<!doctype html><main>Prototype artifact</main>',
          },
        },
      ],
    };

    const markup = renderToStaticMarkup(<App initialState={artifactState} />);

    expect(markup).toContain('Prototype');
    expect(markup).toContain('1 个真实 artifact');
    expect(markup).toContain('空白工作流');
    expect(markup).not.toContain('Prototype artifact');
  });

  it('renders pending proposals as agent chat review cards with canvas diff state', () => {
    const proposalState: WorkbenchState = {
      ...state,
      pendingConfirmation: null,
      pendingProposal: {
        id: 'proposal_1',
        title: 'Add output review node',
        summary: 'Adds one real output review node to the current graph.',
        diffSummary: ['+ Review output node'],
        previewGraph: {
          nodes: [
            ...state.graph.nodes,
            {
              id: 'review',
              nodeType: 'output.review',
              title: 'Review output',
              category: 'Output',
              status: 'queued',
              position: { x: 720, y: 156 },
              provider: null,
              summary: '{}',
            },
          ],
          edges: [
            ...state.graph.edges,
            {
              id: 'edge_save_review',
              from: { nodeId: 'save', port: 'artifact' },
              to: { nodeId: 'review', port: 'artifact' },
              kind: 'artifact',
            },
          ],
        },
      },
    };

    const markup = renderToStaticMarkup(<App initialState={proposalState} />);

    expect(markup).toContain('已生成待审核的图变更提议');
    expect(markup).toContain('Add output review node');
    expect(markup).toContain('查看 Diff');
    expect(markup).toContain('class="node node--add');
    expect(markup).toContain('edge-path edge-path--new');
    expect(markup).toContain('待确认的图变更 — 预览中');
  });

  it('collapses agent tool logs instead of rendering raw transcript in the main chat', () => {
    const logState: WorkbenchState = {
      ...state,
      pendingConfirmation: null,
      chat: {
        messages: [
          ...state.chat.messages,
          {
            id: 'log_1',
            role: 'agent',
            text: "/bin/zsh -lc '/usr/bin/sed -n 1,220p ctx/graph.json'",
            time: '2026-06-16T03:39:00Z',
            kind: 'agent_log:command_execution',
            label: 'command_execution',
            raw: '{"type":"item.completed"}',
          },
          {
            id: 'log_1_started',
            role: 'agent',
            text: "/bin/zsh -lc '/usr/bin/sed -n 1,220p ctx/graph.json'",
            time: '2026-06-16T03:39:00Z',
            kind: 'agent_log:command_execution',
            label: 'command_execution',
          },
          {
            id: 'log_2',
            role: 'agent',
            text: '{"type":"turn.completed","usage":{"input_tokens":96653}}',
            time: '2026-06-16T03:39:01Z',
            kind: 'agent_log:turn_completed',
            label: 'turn.completed',
          },
          {
            id: 'log_3',
            role: 'agent',
            text: '{"type":"thread.started","thread_id":"thread_1"}',
            time: '2026-06-16T03:39:01Z',
            kind: 'agent_log:thread_started',
            label: 'thread.started',
          },
          {
            id: 'log_4',
            role: 'agent',
            text: 'I will inspect the context files.',
            time: '2026-06-16T03:39:01Z',
            kind: 'agent_log:assistant_message',
            label: 'assistant',
          },
          {
            id: 'log_5',
            role: 'agent',
            text: 'Skill descriptions were shortened to fit the 2% skills context budget.',
            time: '2026-06-16T03:39:01Z',
            kind: 'agent_log:error',
            label: 'error',
          },
          {
            id: 'log_6',
            role: 'agent',
            text: '{"changes":[{"path":"/tmp/session/out/reply.json"}]}',
            time: '2026-06-16T03:39:01Z',
            kind: 'agent_log:file_change',
            label: 'file_change',
          },
          {
            id: 'msg_agent',
            role: 'agent',
            text: '你想改哪里？请说明节点、参数或连接。',
            time: '2026-06-16T03:39:02Z',
            kind: 'chat',
            label: 'agent_session',
          },
        ],
      },
    };

    const markup = renderToStaticMarkup(<App initialState={logState} />);

    expect(markup).toContain('工具调用 · 1 次');
    expect(markup).toContain('你想改哪里？请说明节点、参数或连接。');
    expect(markup.match(/class="msg msg--assistant"/g)).toHaveLength(1);
    expect(markup).not.toContain('/bin/zsh');
    expect(markup).not.toContain('I will inspect');
    expect(markup).not.toContain('Skill descriptions were shortened');
    expect(markup).not.toContain('/tmp/session/out/reply.json');
    expect(markup).not.toContain('turn.completed');
    expect(markup).not.toContain('thread.started');
    expect(markup).not.toContain('input_tokens');
  });

  it('renders active agent status and tool calls as one assistant chat turn', () => {
    const activeState: WorkbenchState = {
      ...state,
      pendingConfirmation: null,
      chat: {
        messages: [
          {
            id: 'msg_user_active',
            role: 'user',
            text: '做一个 ComfyUI 工作流',
            time: '2026-06-16T03:39:00Z',
            kind: 'chat',
            label: null,
          },
          {
            id: 'agent-status-agent_session_1',
            role: 'agent',
            text: 'Agent 正在处理请求',
            time: '2026-06-16T03:39:01Z',
            kind: 'agent_status',
            label: 'turn.sent',
          },
          {
            id: 'agent-log-command_1',
            role: 'agent',
            text: "/bin/zsh -lc '/usr/bin/sed -n 1,220p ctx/graph.json'",
            time: '2026-06-16T03:39:02Z',
            kind: 'agent_log:command_execution',
            label: 'command_execution',
          },
        ],
      },
    };

    const markup = renderToStaticMarkup(<App initialState={activeState} />);

    expect(markup).toContain('做一个 ComfyUI 工作流');
    expect(markup).toContain('Agent 正在处理请求');
    expect(markup).toContain('工具调用 · 1 次');
    expect(markup.match(/class="msg msg--assistant"/g)).toHaveLength(1);
    expect(markup).not.toContain('执行日志');
    expect(markup).not.toContain('/bin/zsh');
  });

  it('replaces an active status turn when the final assistant reply arrives', () => {
    const finalState: WorkbenchState = {
      ...state,
      pendingConfirmation: null,
      chat: {
        messages: [
          {
            id: 'msg_user_active',
            role: 'user',
            text: '你好啊',
            time: '2026-06-16T03:39:00Z',
            kind: 'text',
            label: null,
          },
          {
            id: 'agent-status-agent_session_1',
            role: 'agent',
            text: 'Agent 正在执行工具调用',
            time: '2026-06-16T03:39:01Z',
            kind: 'agent_status',
            label: 'runtime.status',
          },
          {
            id: 'agent-log-command_1',
            role: 'agent',
            text: "/bin/zsh -lc '/usr/bin/sed -n 1,220p ctx/graph.json'",
            time: '2026-06-16T03:39:02Z',
            kind: 'agent_log:command_execution',
            label: 'command_execution',
          },
          {
            id: 'msg_agent_final',
            role: 'agent',
            text: '你好！有什么我可以帮你的吗？',
            time: '2026-06-16T03:39:03Z',
            kind: 'chat',
            label: 'agent_session_1',
          },
        ],
      },
    };

    const markup = renderToStaticMarkup(<App initialState={finalState} />);

    expect(markup).toContain('你好！有什么我可以帮你的吗？');
    expect(markup).toContain('工具调用 · 1 次');
    expect(markup.match(/class="msg msg--assistant"/g)).toHaveLength(1);
    expect(markup).not.toContain('Agent 正在执行工具调用');
    expect(markup).not.toContain('/bin/zsh');
  });

  it('applies websocket node state events to visible run state', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'demo',
      run_id: 'run_demo_1',
      seq: 8,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'node.state',
      data: { node_id: 'save', state: 'running' },
    });

    expect(updated.eventSeq).toBe(8);
    expect(updated.graph.nodes.find((node) => node.id === 'save')?.status).toBe('running');
    expect(updated.run.steps.find((step) => step.nodeId === 'save')?.state).toBe('running');
  });

  it('ignores websocket events for another run in the same workspace', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'demo',
      run_id: 'run_other',
      seq: 8,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'node.state',
      data: { node_id: 'save', state: 'running' },
    });

    expect(updated).toBe(state);
  });

  it('applies agent status events even when they use an agent session id', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'demo',
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
      text: 'Drafting graph proposal',
    });
  });

  it('renders internal agent status names as human-readable progress', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'demo',
      run_id: 'agent_session_1',
      seq: 1,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'agent.status',
      data: {
        session_id: 'agent_session_1',
        status: 'turn.sent',
        detail: {},
      },
    });

    expect(updated.chat.messages.at(-1)).toMatchObject({
      text: 'Agent 正在处理请求',
    });
  });

  it('removes active agent status when the agent end event arrives', () => {
    const running = applyRunEvent(state, {
      workspace_id: 'demo',
      run_id: 'agent_session_1',
      seq: 1,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'agent.status',
      data: {
        session_id: 'agent_session_1',
        status: 'turn.sent',
        detail: {},
      },
    });

    const completed = applyRunEvent(running, {
      workspace_id: 'demo',
      run_id: 'agent_session_1',
      seq: 2,
      server_time: '2026-06-12T00:00:02Z',
      ev: 'agent.status.end',
      data: {
        session_id: 'agent_session_1',
        status: 'agent.status.end',
        detail: {},
      },
    });

    expect(completed.chat.messages.some((message) => message.id === 'agent-status-agent_session_1'))
      .toBe(false);
  });

  it('shows an optimistic outgoing user message while the backend turn is running', async () => {
    let resolveFetch: ((response: Response) => void) | undefined;
    const fetchMock = vi.fn(
      () =>
        new Promise<Response>((resolve) => {
          resolveFetch = resolve;
        }),
    );
    vi.stubGlobal('fetch', fetchMock);
    useWorkbenchStore.getState().setInitialState(state);

    const pending = useWorkbenchStore.getState().sendMessage('demo', 'Slow request');

    expect(useWorkbenchStore.getState().state?.chat.messages.at(-1)).toMatchObject({
      id: expect.stringContaining('optimistic-user-'),
      role: 'user',
      text: 'Slow request',
    });

    const nextState: WorkbenchState = {
      ...state,
      chat: {
        messages: [
          ...state.chat.messages,
          {
            id: 'msg_user_slow',
            role: 'user',
            text: 'Slow request',
            time: '2026-06-16T03:39:00Z',
            kind: 'text',
            label: null,
          },
        ],
      },
    };
    resolveFetch?.(jsonResponse(nextState));
    await pending;

    const matching = useWorkbenchStore
      .getState()
      .state?.chat.messages.filter((message) => message.text === 'Slow request');
    expect(matching).toHaveLength(1);
    expect(matching?.[0].id).toBe('msg_user_slow');
  });

  it('posts composer messages through the workspace action API', async () => {
    const nextState: WorkbenchState = {
      ...state,
      chat: {
        messages: [
          ...state.chat.messages,
          { id: 'msg_2', role: 'user', text: 'Make it brighter', time: 'now' },
        ],
      },
    };
    const fetchMock = vi.fn(async () => jsonResponse(nextState));
    vi.stubGlobal('fetch', fetchMock);

    await useWorkbenchStore.getState().sendMessage('demo', 'Make it brighter');

    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/demo/messages', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: 'Make it brighter' }),
    });
    expect(useWorkbenchStore.getState().state?.chat.messages.at(-1)?.text).toBe(
      'Make it brighter',
    );
  });

  it('loads workspace state when persisted message metadata is null', async () => {
    const fetchMock = vi.fn(async () =>
      jsonResponse({
        ...state,
        chat: {
          messages: [
            {
              ...state.chat.messages[0],
              kind: 'text',
              label: null,
              raw: null,
            },
          ],
        },
      }),
    );
    vi.stubGlobal('fetch', fetchMock);

    const loaded = await fetchWorkspaceState('demo');

    expect(loaded.chat.messages[0]).toMatchObject({
      kind: 'text',
      label: null,
      raw: null,
    });
  });

  it('loads workspace history list from the real workspace API shape', async () => {
    const fetchMock = vi.fn(async () =>
      jsonResponse({
        workspaces: [
          {
            id: 'chat_1',
            name: 'Helixflow Workspace',
            versionId: 'ver_1',
            createdAt: '2026-06-16T03:40:00Z',
            updatedAt: '2026-06-16T03:41:00Z',
            firstMessage: '创建文生图工作流',
            messageCount: 3,
          },
        ],
      }),
    );
    vi.stubGlobal('fetch', fetchMock);

    const workspaces = await fetchWorkspaceList();

    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces');
    expect(workspaces[0]).toMatchObject({
      id: 'chat_1',
      firstMessage: '创建文生图工作流',
      messageCount: 3,
    });
  });

  it('renders conversation history separately from version history', () => {
    const markup = renderToStaticMarkup(
      <HistoryPanel
        currentWorkspaceId="chat_1"
        history={state.history}
        onClose={() => undefined}
        onOpenWorkspace={() => undefined}
        open
        workspaceListError={null}
        workspaces={[
          {
            id: 'chat_1',
            name: 'Helixflow Workspace',
            versionId: 'ver_1',
            createdAt: '2026-06-16T03:40:00Z',
            updatedAt: '2026-06-16T03:41:00Z',
            firstMessage: '创建文生图工作流',
            messageCount: 3,
          },
        ]}
      />,
    );

    expect(markup).toContain('对话历史');
    expect(markup).toContain('创建文生图工作流');
    expect(markup).toContain('3 条消息');
    expect(markup).toContain('版本与运行历史');
    expect(markup).toContain('Initial graph');
  });

  it('posts run requests through the workspace action API', async () => {
    const nextState: WorkbenchState = {
      ...state,
      run: { ...state.run, status: 'waiting_confirmation' },
    };
    const fetchMock = vi.fn(async () => jsonResponse(nextState));
    vi.stubGlobal('fetch', fetchMock);

    await useWorkbenchStore.getState().requestRun('demo');

    expect(fetchMock).toHaveBeenCalledWith('/api/workspaces/demo/runs', {
      method: 'POST',
      headers: undefined,
      body: undefined,
    });
    expect(useWorkbenchStore.getState().state?.run.status).toBe('waiting_confirmation');
  });

  it('posts confirmation actions and stores the returned state', async () => {
    const nextState: WorkbenchState = {
      ...state,
      pendingConfirmation: null,
      run: { ...state.run, status: 'succeeded' },
    };
    const fetchMock = vi.fn(async () => jsonResponse(nextState));
    vi.stubGlobal('fetch', fetchMock);

    await useWorkbenchStore.getState().approveConfirmation('demo', 'confirm_1');

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/workspaces/demo/confirmations/confirm_1/approve',
      {
        method: 'POST',
        headers: undefined,
        body: undefined,
      },
    );
    expect(useWorkbenchStore.getState().state?.pendingConfirmation).toBeNull();
    expect(useWorkbenchStore.getState().state?.run.status).toBe('succeeded');
  });
});

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });
}
