import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './app';
import { DirtyNavigationDialog } from './components/dirty-navigation-dialog';
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

  it('routes workspace, create, undo, and restore through one pending dialog', async () => {
    renderer = await renderDirtyApp();
    const topBar = renderer.root.findByType(TopBar);
    const history = renderer.root.findByType(HistoryPanel);
    expect(history.props.migrationBlocked).toBe(true);

    await expectTarget(() => topBar.props.onNewWorkspace(), { kind: 'create_workspace' });
    await expectTarget(() => topBar.props.onUndo(), { kind: 'undo' });
    await expectTarget(() => history.props.onOpenWorkspace('ws_b'), {
      kind: 'workspace',
      workspaceId: 'ws_b',
    });
    await expectTarget(() => history.props.onRestoreVersion('ver_old'), {
      kind: 'restore',
      versionId: 'ver_old',
    });
  });

  it('ignores a second navigation while a decision is pending and preserves edits on cancel', async () => {
    renderer = await renderDirtyApp();
    const topBar = renderer.root.findByType(TopBar);

    await act(async () => topBar.props.onUndo());
    await act(async () => topBar.props.onNewWorkspace());
    expect(dialog().props.target).toEqual({ kind: 'undo' });

    await act(async () => dialog().props.onDecision('cancel'));
    expect(dialog().props.target).toBeNull();
    expect(useWorkbenchStore.getState().editSession).toEqual(dirtySession());
  });

  it('discards before workspace navigation', async () => {
    renderer = await renderDirtyApp();
    const history = renderer.root.findByType(HistoryPanel);

    await act(async () => history.props.onOpenWorkspace('ws_b'));
    await act(async () => {
      dialog().props.onDecision('discard');
      await flushActions();
    });

    expect(dialog().props.target).toBeNull();
    expect(useWorkbenchStore.getState().editSession).toBeNull();
  });

  it('commits before undo and keeps the dialog plus edits when commit fails', async () => {
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

    await act(async () => topBar.props.onUndo());
    await act(async () => {
      dialog().props.onDecision('commit');
      await flushActions();
    });

    expect(fetchMock.mock.calls.map(([input]) => String(input))).toEqual(expect.arrayContaining([
      '/api/workspaces/ws_a/versions/ops',
      '/api/workspaces/ws_a/versions/undo',
    ]));
    expect(dialog().props.target).toBeNull();
    expect(useWorkbenchStore.getState().editSession).toBeNull();

    const failingFetch = vi.fn(async (input: RequestInfo | URL) => {
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
    vi.stubGlobal('fetch', failingFetch);
    await act(async () => useWorkbenchStore.setState({
      state: stateForVersion('ver_a'),
      editSession: dirtySession(),
    }));
    await act(async () => topBar.props.onUndo());
    await act(async () => {
      dialog().props.onDecision('commit');
      await flushActions();
    });

    expect(dialog().props.target).toEqual({ kind: 'undo' });
    expect(useWorkbenchStore.getState().editSession).toEqual(dirtySession());
    expect(failingFetch.mock.calls.some(([input]) => String(input).endsWith('/versions/undo')))
      .toBe(false);
  });

  async function renderDirtyApp(): Promise<ReactTestRenderer> {
    let next!: ReactTestRenderer;
    await act(async () => {
      next = create(<App initialState={stateForVersion('ver_a')} />);
      await flushActions();
    });
    await act(async () => useWorkbenchStore.setState({ editSession: dirtySession() }));
    return next;
  }

  async function expectTarget(trigger: () => void, target: unknown) {
    await act(async () => trigger());
    expect(dialog().props.target).toEqual(target);
    await act(async () => dialog().props.onDecision('cancel'));
  }

  function dialog() {
    if (!renderer) throw new Error('renderer is not mounted');
    return renderer.root.findByType(DirtyNavigationDialog);
  }
});

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
    history: [
      { id: 'ver_old', kind: 'version', label: 'Old', time: 'unix:1', summary: 'old' },
      { id: versionId, kind: 'version', label: 'Current', time: 'unix:2', summary: 'current' },
    ],
    pendingConfirmation: null,
    pendingProposal: null,
    workflowGraph: { schema_version: 1, nodes: {}, edges: [] },
  };
}
