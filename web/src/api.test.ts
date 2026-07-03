import { afterEach, describe, expect, it, vi } from 'vitest';
import { connectWorkspaceEvents } from './api';
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
    expect(fetch).toHaveBeenCalledWith('/api/workspaces/ws_test/events?afterSeq=0');
    expect(fetch).toHaveBeenCalledWith('/api/canvases/ws_test/ticket', { method: 'POST' });
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
});

function stubBrowser(): void {
  vi.stubGlobal('window', { location: { protocol: 'http:', host: 'localhost' } });
  vi.stubGlobal('WebSocket', MockWebSocket);
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
