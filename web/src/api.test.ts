import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  applyVersionMigration,
  connectWorkspaceEvents,
  dryRunVersionMigration,
} from './api';
import type { RunEventEnvelope } from './types';
import { jsonResponse, runEvent as baseRunEvent, waitUntil } from './test-utils';

type Listener = (event: { data?: string }) => void;

class MockWebSocket {
  static sockets: MockWebSocket[] = [];

  readonly listeners = new Map<string, Listener[]>();

  constructor(readonly url: string) {
    MockWebSocket.sockets.push(this);
  }

  addEventListener(type: string, listener: Listener): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  close(): void {
    this.emit('close', {});
  }

  emit(type: string, event: { data?: string }): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }

  emitMessage(event: RunEventEnvelope): void {
    this.emit('message', { data: JSON.stringify(event) });
  }
}

describe('connectWorkspaceEvents', () => {
  afterEach(() => {
    MockWebSocket.sockets = [];
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('requests a canvas ticket before opening the websocket when required', async () => {
    stubBrowser();
    vi.stubGlobal('fetch', ticketFetch('required'));

    const cleanup = connectWorkspaceEvents('ws_test', {
      getLastSeq: () => 0,
      onEvent: () => undefined,
      onStatus: () => undefined,
    });

    await waitUntil(() => MockWebSocket.sockets.length === 1);
    expect(fetch).toHaveBeenCalledWith(
      '/api/workspaces/ws_test/events?afterSeq=0',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(fetch).toHaveBeenCalledWith(
      '/api/canvases/ws_test/ticket',
      expect.objectContaining({ method: 'POST', signal: expect.any(AbortSignal) }),
    );
    expect(MockWebSocket.sockets[0]?.url).toBe(
      'ws://localhost/ws?workspace_id=ws_test&ticket=ticket_1',
    );
    cleanup();
  });

  it('fetches missing events before applying a websocket event with a seq gap', async () => {
    stubBrowser();
    let lastSeq = 1;
    const applied: number[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input);
        if (url === '/api/canvases/ws_test/ticket') {
          return jsonResponse({ mode: 'disabled' });
        }
        if (url === '/api/workspaces/ws_test/events?afterSeq=1') {
          return jsonResponse({ events: [nodeStateEvent(2)] });
        }
        if (url === '/api/workspaces/ws_test/events?afterSeq=2') {
          return jsonResponse({ events: [nodeStateEvent(3)] });
        }
        return new Response('{}', { status: 404 });
      }),
    );

    const cleanup = connectWorkspaceEvents('ws_test', {
      getLastSeq: () => lastSeq,
      onEvent: (event) => {
        applied.push(event.seq);
        lastSeq = event.seq;
      },
      onStatus: () => undefined,
    });

    await waitUntil(() => MockWebSocket.sockets.length === 1 && applied.join(',') === '2');
    MockWebSocket.sockets[0]?.emitMessage(nodeStateEvent(4));

    await waitUntil(() => applied.join(',') === '2,3,4');
    cleanup();
  });

  it('does not open a websocket when cleanup wins the ticket request race', async () => {
    stubBrowser();
    let resolveTicket!: (response: Response) => void;
    const ticket = new Promise<Response>((resolve) => {
      resolveTicket = resolve;
    });
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input);
        if (url.includes('/events?')) return jsonResponse({ events: [] });
        if (url.includes('/ticket')) return ticket;
        return new Response('{}', { status: 404 });
      }),
    );

    const cleanup = connectWorkspaceEvents('ws_test', {
      getLastSeq: () => 0,
      onEvent: () => undefined,
      onStatus: () => undefined,
    });
    await waitUntil(() => vi.mocked(fetch).mock.calls.length === 2);
    cleanup();
    resolveTicket(jsonResponse({ mode: 'disabled' }));
    await Promise.resolve();
    await Promise.resolve();

    expect(MockWebSocket.sockets).toHaveLength(0);
  });

  it('ignores presence envelopes from another workspace', async () => {
    stubBrowser();
    vi.stubGlobal('fetch', ticketFetch('disabled'));
    const presences: unknown[] = [];
    const cleanup = connectWorkspaceEvents('ws_test', {
      getLastSeq: () => 0,
      onEvent: () => undefined,
      onPresence: (presence) => presences.push(presence),
      onStatus: () => undefined,
    });
    await waitUntil(() => MockWebSocket.sockets.length === 1);
    MockWebSocket.sockets[0]?.emitMessage({
      ...baseRunEvent('presence', 1, 'canvas.presence'),
      workspace_id: 'ws_other',
      data: {
        actor: { actorId: 'other', displayName: 'Other' },
        cursor: { x: 1, y: 2 },
      },
    });

    expect(presences).toEqual([]);
    cleanup();
  });
});

describe('version migration API', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('parses the server dry-run report and encodes route identifiers', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(migrationReport())));

    const report = await dryRunVersionMigration('ws/a', 'ver b');

    expect(report.status).toBe('migratable');
    expect(fetch).toHaveBeenCalledWith(
      '/api/workspaces/ws%2Fa/versions/ver%20b/migration/dry-run',
      { method: 'POST', signal: undefined },
    );
  });

  it('sends all report preconditions when applying a migration', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => jsonResponse({
        targetVersionId: 'ver_v2',
        targetGraphHash: 'sha256:target',
        replayed: false,
        workspaceState: emptyWorkbenchState(),
      })),
    );

    const report = migrationReport();
    const result = await applyVersionMigration('ws_1', 'ver_v1', {
      operationId: 'op_1',
      reportHash: report.reportHash,
      sourceGraphHash: report.sourceGraphHash,
      catalogRevision: report.catalogRevision,
      workspaceConnectorId: report.workspaceConnectorId,
      migrationVersion: report.migrationVersion,
    });

    expect(result.targetVersionId).toBe('ver_v2');
    const options = vi.mocked(fetch).mock.calls[0]?.[1];
    expect(JSON.parse(String(options?.body))).toEqual({
      operationId: 'op_1',
      reportHash: 'sha256:report',
      sourceGraphHash: 'sha256:source',
      catalogRevision: 'catalog-1',
      workspaceConnectorId: 'atlas',
      migrationVersion: '1',
    });
  });
});

function stubBrowser(): void {
  vi.stubGlobal('window', { location: { protocol: 'http:', host: 'localhost' } });
  vi.stubGlobal('WebSocket', MockWebSocket);
}

function migrationReport() {
  return {
    status: 'migratable' as const,
    migrationVersion: '1',
    workspaceId: 'ws_1',
    sourceVersionId: 'ver_v1',
    sourceGraphHash: 'sha256:source',
    sourceSchemaVersion: 1,
    catalogRevision: 'catalog-1',
    workspaceConnectorId: 'atlas',
    applyEnabled: true,
    reportHash: 'sha256:report',
    nodes: [],
  };
}

function emptyWorkbenchState() {
  return {
    eventSeq: 0,
    workspace: {
      id: 'ws_1',
      name: 'Migration',
      versionId: 'ver_v2',
      updatedAt: '2026-07-27T00:00:00Z',
    },
    providers: {
      defaultProvider: 'atlas',
      selectedProvider: 'atlas',
      runtimeProviders: [],
      workflowBackends: [],
      apiConnectors: [],
    },
    chat: { messages: [] },
    graph: { nodes: [], edges: [] },
    run: null,
    outputs: [],
    history: [],
    pendingConfirmation: null,
    pendingProposal: null,
  };
}

function ticketFetch(mode: 'disabled' | 'required') {
  return vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url === '/api/workspaces/ws_test/events?afterSeq=0') {
      return jsonResponse({ events: [] });
    }
    if (url === '/api/canvases/ws_test/ticket') {
      return jsonResponse(
        mode === 'required'
          ? { mode, ticket: 'ticket_1', expiresAt: 200 }
          : { mode },
      );
    }
    return new Response('{}', { status: 404 });
  });
}

function nodeStateEvent(seq: number): RunEventEnvelope {
  return {
    ...baseRunEvent('run_1', seq, 'node.state'),
    data: { node_id: 'video', state: 'running' },
  };
}
