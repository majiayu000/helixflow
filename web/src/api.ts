import {
  RunEventEnvelopeSchema,
  WorkbenchStateSchema,
  type RunEventEnvelope,
  type WorkbenchState,
} from './types';

export type ConnectionStatus = 'connecting' | 'live' | 'offline';

type EventHandlers = {
  onEvent: (event: RunEventEnvelope) => void;
  onStatus: (status: ConnectionStatus) => void;
};

export async function fetchWorkspaceState(workspaceId: string): Promise<WorkbenchState> {
  const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/state`);
  if (!response.ok) {
    throw new Error(`workspace state request failed: ${response.status}`);
  }

  return WorkbenchStateSchema.parse(await response.json());
}

export function connectWorkspaceEvents(
  workspaceId: string,
  handlers: EventHandlers,
): () => void {
  if (typeof WebSocket === 'undefined') {
    handlers.onStatus('offline');
    return () => undefined;
  }

  handlers.onStatus('connecting');
  const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
  const socket = new WebSocket(
    `${protocol}://${window.location.host}/ws?workspace_id=${encodeURIComponent(workspaceId)}`,
  );

  socket.addEventListener('open', () => handlers.onStatus('live'));
  socket.addEventListener('close', () => handlers.onStatus('offline'));
  socket.addEventListener('error', () => handlers.onStatus('offline'));
  socket.addEventListener('message', (message) => {
    try {
      const parsed = RunEventEnvelopeSchema.safeParse(JSON.parse(String(message.data)));
      if (parsed.success) {
        handlers.onEvent(parsed.data);
      } else {
        handlers.onStatus('offline');
      }
    } catch {
      handlers.onStatus('offline');
    }
  });

  return () => socket.close();
}
