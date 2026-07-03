import type { RunEventEnvelope } from './types';

export function runEvent(runId: string, seq: number, ev: string): RunEventEnvelope {
  return {
    workspace_id: 'ws_test',
    run_id: runId,
    seq,
    server_time: '2026-07-02T00:00:01Z',
    ev,
    data: {},
  };
}

export function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

export async function waitUntil(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  throw new Error('condition was not met');
}
