import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './app';
import { GraphCanvas } from './components/graph-canvas';
import { HistoryPanel } from './components/run-panels';
import { TopBar } from './components/top-bar';
import { useWorkbenchStore } from './store';
import { flushActions, jsonResponse } from './test-utils';
import type { ManualEditSession, WorkbenchState } from './types';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('App dirty navigation integration', () => {
  let renderer: ReactTestRenderer | null = null;

  beforeEach(() => {
    useWorkbenchStore.setState({
      status: 'idle',
      error: null,
      canvasStatus: 'idle',
      canvasError: null,
      canvas: null,
      state: null,
      editSession: null,
      activeWorkspaceId: null,
      selectedCanvasNodeIds: [],
      presenceByActor: {},
    });
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('/events')) return jsonResponse({ events: [] });
      if (url === '/api/workspaces') return jsonResponse([]);
      return new Response('{}', { status: 404 });
    }));
  });

  afterEach(async () => {
    if (renderer) await act(async () => renderer?.unmount());
    renderer = null;
    vi.unstubAllGlobals();
  });

  it('saves leftover canvas edits before undo instead of asking to commit', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('/events')) return jsonResponse({ events: [] });
      if (url === '/api/workspaces') return jsonResponse([]);
      if (url.endsWith('/versions/ops')) return jsonResponse(stateForVersion('ver_committed'));
      if (url.endsWith('/versions/undo')) return jsonResponse(stateForVersion('ver_undo'));
      return new Response('{}', { status: 404 });
    });
    vi.stubGlobal('fetch', fetchMock);
    renderer = await renderDirtyApp();
    const topBar = renderer.root.findByType(TopBar);

    await act(async () => {
      topBar.props.onUndo();
      await flushActions();
    });

    expect(fetchMock.mock.calls.map(([input]) => String(input))).toEqual(expect.arrayContaining([
      '/api/workspaces/ws_a/versions/ops',
      '/api/workspaces/ws_a/versions/undo',
    ]));
    expect(useWorkbenchStore.getState().editSession).toBeNull();
    expect(useWorkbenchStore.getState().state?.workspace.versionId).toBe('ver_undo');
  });

  it('saves leftover canvas edits before switching workspace', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('/events')) return jsonResponse({ events: [] });
      if (url === '/api/workspaces') return jsonResponse([]);
      if (url.endsWith('/versions/ops')) return jsonResponse(stateForVersion('ver_committed'));
      return new Response('{}', { status: 404 });
    });
    vi.stubGlobal('fetch', fetchMock);
    renderer = await renderDirtyApp();
    const history = renderer.root.findByType(HistoryPanel);

    expect(history.props.migrationBlocked).toBe(true);
    await act(async () => {
      history.props.onOpenWorkspace('ws_b');
      await flushActions();
    });

    expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/workspaces/ws_a/versions/ops'))
      .toBe(true);
    expect(useWorkbenchStore.getState().editSession).toBeNull();
  });

  it('stays on the current workspace when leftover edits fail to save', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('/events')) return jsonResponse({ events: [] });
      if (url === '/api/workspaces') return jsonResponse([]);
      if (url.endsWith('/versions/ops')) {
        return new Response(JSON.stringify({ error: 'commit failed' }), {
          status: 503,
          headers: { 'content-type': 'application/json' },
        });
      }
      return new Response('{}', { status: 404 });
    });
    vi.stubGlobal('fetch', fetchMock);
    renderer = await renderDirtyApp();
    const topBar = renderer.root.findByType(TopBar);

    await act(async () => {
      topBar.props.onUndo();
      await flushActions();
    });

    expect(fetchMock.mock.calls.some(([input]) => String(input).endsWith('/versions/undo')))
      .toBe(false);
    expect(useWorkbenchStore.getState().state?.workspace.versionId).toBe('ver_a');
    expect(useWorkbenchStore.getState().editSession).toBeNull();
  });

  it('sends an explicit run mode from the Agent Run surface', async () => {
    const fetchMock = agentMessageFetch('run_request');
    vi.stubGlobal('fetch', fetchMock);
    renderer = await renderCleanApp();

    await act(async () => {
      renderer!.root.findByType(TopBar).props.onAgentRun();
      await flushActions();
    });

    expect(messageRequestBody(fetchMock).turnMode).toBe('run_request');
  });

  it('sends an explicit modify mode from the selected-node Agent action', async () => {
    const fetchMock = agentMessageFetch('modify_workflow');
    vi.stubGlobal('fetch', fetchMock);
    renderer = await renderCleanApp();

    await act(async () => {
      renderer!.root.findByType(GraphCanvas).props.onRequestNodeProposal('video');
      await flushActions();
    });

    expect(messageRequestBody(fetchMock).turnMode).toBe('modify_workflow');
  });

  async function renderDirtyApp(): Promise<ReactTestRenderer> {
    let next!: ReactTestRenderer;
    await act(async () => {
      next = create(<App initialState={stateForVersion('ver_a')} />);
      await flushActions();
    });
    await act(async () => useWorkbenchStore.setState({
      state: stateForVersion('ver_a'),
      editSession: dirtySession(),
    }));
    return next;
  }

  async function renderCleanApp(): Promise<ReactTestRenderer> {
    let next!: ReactTestRenderer;
    await act(async () => {
      next = create(<App initialState={stateForVersion('ver_a')} />);
      await flushActions();
    });
    return next;
  }
});

function agentMessageFetch(turnMode: 'run_request' | 'modify_workflow') {
  return vi.fn(async (input: RequestInfo | URL, _init?: RequestInit) => {
    const url = String(input);
    if (url.includes('/events')) return jsonResponse({ events: [] });
    if (url === '/api/workspaces') return jsonResponse([]);
    if (url.endsWith('/messages')) {
      return jsonResponse({
        conversationId: 'conv_agent_action',
        turnId: 'turn_agent_action',
        turnStatus: 'succeeded',
        turnMode,
        messages: [{
          id: 'msg_agent_action',
          role: 'agent',
          kind: 'chat',
          text: 'Agent action completed.',
          time: 'unix:1',
          conversationId: 'conv_agent_action',
          turnId: 'turn_agent_action',
        }],
        proposal: null,
        run: null,
        pendingConfirmation: null,
      });
    }
    return new Response('{}', { status: 404 });
  });
}

function messageRequestBody(fetchMock: ReturnType<typeof agentMessageFetch>) {
  const call = fetchMock.mock.calls.find(([input]) => String(input).endsWith('/messages'));
  if (!call) throw new Error('workspace message request was not sent');
  return JSON.parse(String(call[1]?.body)) as { turnMode?: string };
}

function dirtySession(): ManualEditSession {
  return {
    baseVersionId: 'ver_a',
    idempotencyKey: 'canvas_op_dirty',
    source: 'user',
    startedAt: '2026-07-11T00:00:00Z',
    ops: [{ op: 'move_node', id: 'video', pos: [120, 80] }],
  };
}

function stateForVersion(versionId: string): WorkbenchState {
  return {
    eventSeq: 0,
    workspace: { id: 'ws_a', name: 'A', versionId, updatedAt: '2026-07-11T00:00:00Z' },
    providers: {
      defaultProvider: 'mock', selectedProvider: 'mock', runtimeProviders: [],
      workflowBackends: [], apiConnectors: [],
    },
    chat: { messages: [] },
    graph: {
      nodes: [{
        id: 'video', nodeType: 'video.text_to_video', title: 'Video', category: 'Video',
        status: 'queued', position: { x: 0, y: 0 }, provider: 'mock', summary: 'Video',
      }],
      edges: [],
    },
    run: {
      id: 'run_a', label: 'Run', status: 'queued',
      steps: [{ nodeId: 'video', title: 'Video', state: 'queued', provider: 'mock' }],
      cost: { estimate: 0, actual: 0, currency: 'USD' },
    },
    outputs: [],
    imageProcessingJobs: [],
    history: [
      { id: 'ver_old', kind: 'version', label: 'Old', time: 'unix:1', summary: 'old' },
      { id: versionId, kind: 'version', label: 'Current', time: 'unix:2', summary: 'current' },
    ],
    pendingConfirmation: null,
    pendingProposal: null,
    workflowGraph: { schema_version: 1, nodes: {}, edges: [] },
  };
}
