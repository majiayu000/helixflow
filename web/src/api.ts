import {
  CanvasEventsResponseSchema,
  CanvasOpsResponseSchema,
  CanvasPresenceResponseSchema,
  CanvasSnapshotResponseSchema,
  CanvasSocketEventSchema,
  RunEventEnvelopeSchema,
  WorkspaceListSchema,
  WorkbenchStateSchema,
  type CanvasActor,
  type CanvasDocument,
  type CanvasOpEnvelope,
  type CanvasOpKind,
  type CanvasSocketEvent,
  type RunEventEnvelope,
  type WorkbenchState,
  type WorkspaceSummary,
} from './types';

export type ConnectionStatus = 'connecting' | 'live' | 'offline';

type EventHandlers = {
  onEvent: (event: RunEventEnvelope) => void;
  onStatus: (status: ConnectionStatus) => void;
};

type CanvasEventHandlers = {
  onEvent: (event: CanvasSocketEvent) => void;
  onStatus: (status: ConnectionStatus) => void;
};

export type CanvasOpDraft = {
  baseSeq: number;
  actor: CanvasActor;
  kind: CanvasOpKind;
  payload: unknown;
  idempotencyKey: string;
};

export type CanvasPresenceDraft = {
  actorId: string;
  cursor?: unknown;
  selection?: unknown;
  viewport?: unknown;
};

export async function fetchWorkspaceState(workspaceId: string): Promise<WorkbenchState> {
  const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/state`);
  if (!response.ok) {
    throw new Error(`workspace state request failed: ${response.status}`);
  }

  return WorkbenchStateSchema.parse(await response.json());
}

export async function fetchWorkspaceList(): Promise<WorkspaceSummary[]> {
  const response = await fetch('/api/workspaces');
  if (!response.ok) {
    throw new Error(`workspace list request failed: ${response.status}`);
  }

  return WorkspaceListSchema.parse(await response.json()).workspaces;
}

export async function fetchWorkspaceCanvas(workspaceId: string): Promise<CanvasDocument> {
  const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/canvas`);
  if (!response.ok) {
    throw new Error(await errorMessage(response, 'workspace canvas request failed'));
  }

  return CanvasSnapshotResponseSchema.parse(await response.json()).canvas;
}

export async function postCanvasOps(
  canvasId: string,
  ops: CanvasOpDraft[],
): Promise<{ canvas: CanvasDocument; ops: CanvasOpEnvelope[]; runs: unknown[] }> {
  const response = await fetch(`/api/canvases/${encodeURIComponent(canvasId)}/ops`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ops }),
  });
  if (!response.ok) {
    throw new Error(await errorMessage(response, 'canvas ops request failed'));
  }

  return CanvasOpsResponseSchema.parse(await response.json());
}

export async function fetchCanvasEvents(
  canvasId: string,
  afterSeq = 0,
): Promise<CanvasOpEnvelope[]> {
  const response = await fetch(
    `/api/canvases/${encodeURIComponent(canvasId)}/events?afterSeq=${encodeURIComponent(
      String(afterSeq),
    )}`,
  );
  if (!response.ok) {
    throw new Error(await errorMessage(response, 'canvas events request failed'));
  }

  return CanvasEventsResponseSchema.parse(await response.json()).ops;
}

export async function postCanvasPresence(
  canvasId: string,
  presence: CanvasPresenceDraft,
): Promise<unknown> {
  const response = await fetch(`/api/canvases/${encodeURIComponent(canvasId)}/presence`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(presence),
  });
  if (!response.ok) {
    throw new Error(await errorMessage(response, 'canvas presence request failed'));
  }

  return CanvasPresenceResponseSchema.parse(await response.json()).presence;
}

export async function postWorkspaceMessage(
  workspaceId: string,
  text: string,
): Promise<WorkbenchState> {
  return postWorkspaceAction(`/api/workspaces/${encodeURIComponent(workspaceId)}/messages`, {
    text,
  });
}

export async function requestWorkspaceRun(workspaceId: string): Promise<WorkbenchState> {
  return postWorkspaceAction(`/api/workspaces/${encodeURIComponent(workspaceId)}/runs`);
}

export async function applyWorkspaceProposal(
  workspaceId: string,
  proposalId: string,
): Promise<WorkbenchState> {
  return postWorkspaceAction(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/proposals/${encodeURIComponent(
      proposalId,
    )}/apply`,
  );
}

export async function dismissWorkspaceProposal(
  workspaceId: string,
  proposalId: string,
): Promise<WorkbenchState> {
  return postWorkspaceAction(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/proposals/${encodeURIComponent(
      proposalId,
    )}/dismiss`,
  );
}

export async function approveWorkspaceConfirmation(
  workspaceId: string,
  confirmationId: string,
): Promise<WorkbenchState> {
  return postWorkspaceAction(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/confirmations/${encodeURIComponent(
      confirmationId,
    )}/approve`,
  );
}

export async function holdWorkspaceConfirmation(
  workspaceId: string,
  confirmationId: string,
): Promise<WorkbenchState> {
  return postWorkspaceAction(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/confirmations/${encodeURIComponent(
      confirmationId,
    )}/hold`,
  );
}

async function postWorkspaceAction(path: string, body?: unknown): Promise<WorkbenchState> {
  const response = await fetch(path, {
    method: 'POST',
    headers: body ? { 'Content-Type': 'application/json' } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!response.ok) {
    throw new Error(await errorMessage(response, 'workspace action failed'));
  }

  return WorkbenchStateSchema.parse(await response.json());
}

async function errorMessage(response: Response, fallback: string): Promise<string> {
  try {
    const body = (await response.json()) as { error?: unknown };
    if (typeof body.error === 'string' && body.error.length > 0) {
      return body.error;
    }
  } catch {
    // Keep the transport fallback when the server returned non-JSON.
  }
  return `${fallback}: ${response.status}`;
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

export function connectCanvasEvents(
  canvasId: string,
  handlers: CanvasEventHandlers,
): () => void {
  if (typeof WebSocket === 'undefined') {
    handlers.onStatus('offline');
    return () => undefined;
  }

  handlers.onStatus('connecting');
  const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
  const socket = new WebSocket(
    `${protocol}://${window.location.host}/api/canvases/${encodeURIComponent(canvasId)}/ws`,
  );

  socket.addEventListener('open', () => handlers.onStatus('live'));
  socket.addEventListener('close', () => handlers.onStatus('offline'));
  socket.addEventListener('error', () => handlers.onStatus('offline'));
  socket.addEventListener('message', (message) => {
    try {
      const parsed = CanvasSocketEventSchema.safeParse(JSON.parse(String(message.data)));
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
