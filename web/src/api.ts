import {
  CanvasDocumentSchema,
  CanvasPresenceSchema,
  CanvasTicketResponseSchema,
  RunConfirmationResponseSchema,
  RunEventEnvelopeSchema,
  WorkspaceEventsSchema,
  WorkflowGraphSchema,
  WorkspaceSummarySchema,
  WorkspaceMessageResponseSchema,
  WorkbenchStateSchema,
  NodeCatalogSchema,
  type CanvasDocument,
  type CanvasCommentOpInput,
  type CanvasPresence,
  type RunConfirmationResponse,
  type RunEventEnvelope,
  type CanvasMessageContext,
  type LayoutPositionUpdate,
  type ManualProposalInput,
  type NodeCatalog,
  type WorkflowGraph,
  type WorkspaceSummary,
  type WorkspaceMessageResponse,
  type WorkbenchState,
} from './types';
import { manualProposalWithIdempotency } from './workbench-edit-session';

export type ConnectionStatus = 'connecting' | 'live' | 'offline';

type EventHandlers = {
  onEvent: (event: RunEventEnvelope) => void;
  onPresence?: (presence: CanvasPresence) => void;
  onStatus: (status: ConnectionStatus) => void;
  getLastSeq?: () => number;
};

const RECONNECT_DELAY_MS = 1000;

export async function fetchWorkspaceState(workspaceId: string): Promise<WorkbenchState> {
  const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/state`);
  if (!response.ok) {
    throw new Error(`workspace state request failed: ${response.status}`);
  }

  return WorkbenchStateSchema.parse(await response.json());
}

export async function fetchWorkspaceCanvas(workspaceId: string): Promise<CanvasDocument> {
  const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/canvas`);
  if (!response.ok) {
    throw new Error(`workspace canvas request failed: ${response.status}`);
  }

  return CanvasDocumentSchema.parse(await response.json());
}

export async function fetchWorkspaceEvents(
  workspaceId: string,
  afterSeq: number,
): Promise<RunEventEnvelope[]> {
  const safeAfterSeq = Number.isFinite(afterSeq) ? Math.max(0, Math.trunc(afterSeq)) : 0;
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/events?afterSeq=${safeAfterSeq}`,
  );
  if (!response.ok) {
    throw new Error(`workspace events request failed: ${response.status}`);
  }

  return WorkspaceEventsSchema.parse(await response.json()).events;
}

export async function requestCanvasTicket(workspaceId: string): Promise<string | null> {
  const response = await fetch(`/api/canvases/${encodeURIComponent(workspaceId)}/ticket`, {
    method: 'POST',
  });
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `canvas ticket request failed: ${response.status}`;
    throw new Error(message);
  }
  const parsed = CanvasTicketResponseSchema.parse(body);
  if (parsed.mode === 'required' && !parsed.ticket) {
    throw new Error('canvas ticket response did not include a ticket');
  }
  return parsed.ticket ?? null;
}

export async function submitCanvasCommentOp(
  workspaceId: string,
  input: CanvasCommentOpInput,
): Promise<CanvasDocument> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/canvas/comments/ops`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    },
  );
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `canvas comment request failed: ${response.status}`;
    throw new Error(message);
  }

  return CanvasDocumentSchema.parse(body);
}

export async function sendCanvasPresence(
  workspaceId: string,
  input: CanvasPresence,
): Promise<void> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/canvas/presence`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    },
  );
  if (!response.ok) {
    throw new Error(`canvas presence request failed: ${response.status}`);
  }
}

export async function fetchWorkspaces(): Promise<WorkspaceSummary[]> {
  const response = await fetch('/api/workspaces');
  if (!response.ok) {
    throw new Error(`workspace list request failed: ${response.status}`);
  }

  return WorkspaceSummarySchema.array().parse(await response.json());
}

export async function fetchNodeCatalog(): Promise<NodeCatalog> {
  const response = await fetch('/api/registry/catalog');
  if (!response.ok) {
    throw new Error(`node catalog request failed: ${response.status}`);
  }

  return NodeCatalogSchema.parse(await response.json());
}

export async function createWorkspace(name?: string): Promise<WorkspaceSummary> {
  const response = await fetch('/api/workspaces', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `workspace create request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkspaceSummarySchema.parse(body);
}

export async function selectWorkspaceProvider(
  workspaceId: string,
  providerId: string,
): Promise<WorkbenchState> {
  const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/provider`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ providerId }),
  });
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `workspace provider request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkbenchStateSchema.parse(body);
}

export async function sendWorkspaceMessage(
  workspaceId: string,
  input: {
    baseVersionId: string;
    userMessage: string;
    graph: WorkflowGraph;
    canvasContext?: CanvasMessageContext;
  },
): Promise<WorkspaceMessageResponse> {
  const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/messages`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(input),
  });
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `workspace message request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkspaceMessageResponseSchema.parse(body);
}

export async function confirmWorkspaceRun(
  workspaceId: string,
  runId: string,
): Promise<RunConfirmationResponse> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/runs/${encodeURIComponent(runId)}/confirm`,
    {
      method: 'POST',
    },
  );
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `run confirmation request failed: ${response.status}`;
    throw new Error(message);
  }

  return RunConfirmationResponseSchema.parse(body);
}

export async function queueWorkspaceRun(
  workspaceId: string,
  options: { forceRerun?: boolean } = {},
): Promise<RunConfirmationResponse> {
  const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/runs`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ forceRerun: Boolean(options.forceRerun) }),
  });
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `run queue request failed: ${response.status}`;
    throw new Error(message);
  }

  return RunConfirmationResponseSchema.parse(body);
}

export async function interruptRun(runId: string): Promise<RunConfirmationResponse> {
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}/interrupt`, {
    method: 'POST',
  });
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `run interrupt request failed: ${response.status}`;
    throw new Error(message);
  }

  return RunConfirmationResponseSchema.parse(body);
}

export async function exportWorkflowVersion(versionId: string): Promise<WorkflowGraph> {
  const response = await fetch(`/api/versions/${encodeURIComponent(versionId)}/export`);
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `workflow export request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkflowGraphSchema.parse(body);
}

export async function undoWorkspaceVersion(workspaceId: string): Promise<WorkbenchState> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/versions/undo`,
    {
      method: 'POST',
    },
  );
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `workspace undo request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkbenchStateSchema.parse(body);
}

export async function restoreWorkspaceVersion(
  workspaceId: string,
  versionId: string,
): Promise<WorkbenchState> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/versions/${encodeURIComponent(versionId)}/restore`,
    {
      method: 'POST',
    },
  );
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `workspace restore request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkbenchStateSchema.parse(body);
}

export async function saveWorkspaceLayout(
  workspaceId: string,
  input: { baseVersionId: string; positions: LayoutPositionUpdate[] },
): Promise<WorkbenchState> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/versions/layout`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    },
  );
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `workspace layout request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkbenchStateSchema.parse(body);
}

export async function selectOutput(outputId: string): Promise<WorkbenchState> {
  const response = await fetch(`/api/outputs/${encodeURIComponent(outputId)}/select`, {
    method: 'POST',
  });
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `output select request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkbenchStateSchema.parse(body);
}

export async function acceptOutput(outputId: string): Promise<WorkbenchState> {
  const response = await fetch(`/api/outputs/${encodeURIComponent(outputId)}/accept`, {
    method: 'POST',
  });
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `output accept request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkbenchStateSchema.parse(body);
}

export async function rejectOutput(outputId: string, rerun = false): Promise<WorkbenchState> {
  const response = await fetch(`/api/outputs/${encodeURIComponent(outputId)}/reject`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ rerun }),
  });
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `output reject request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkbenchStateSchema.parse(body);
}

export async function holdWorkspaceRun(
  workspaceId: string,
  runId: string,
): Promise<RunConfirmationResponse> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/runs/${encodeURIComponent(runId)}/hold`,
    {
      method: 'POST',
    },
  );
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `run hold request failed: ${response.status}`;
    throw new Error(message);
  }

  return RunConfirmationResponseSchema.parse(body);
}

export async function applyWorkspaceProposal(
  workspaceId: string,
  proposalId: string,
): Promise<WorkbenchState> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/proposals/${encodeURIComponent(proposalId)}/apply`,
    {
      method: 'POST',
    },
  );
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `proposal apply request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkbenchStateSchema.parse(body);
}

export async function createManualWorkspaceProposal(
  workspaceId: string,
  input: ManualProposalInput,
): Promise<WorkbenchState> {
  const payload = manualProposalWithIdempotency(input);
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/versions/ops`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(payload),
    },
  );
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `manual edit request failed: ${response.status}`;
    const opIndex =
      body && typeof body === 'object' && 'opIndex' in body ? ` (op ${body.opIndex})` : '';
    throw new Error(`${message}${opIndex}`);
  }

  return WorkbenchStateSchema.parse(body);
}

export async function dismissWorkspaceProposal(
  workspaceId: string,
  proposalId: string,
): Promise<WorkbenchState> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/proposals/${encodeURIComponent(proposalId)}/dismiss`,
    {
      method: 'POST',
    },
  );
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `proposal dismiss request failed: ${response.status}`;
    throw new Error(message);
  }

  return WorkbenchStateSchema.parse(body);
}

export function connectWorkspaceEvents(
  workspaceId: string,
  handlers: EventHandlers,
): () => void {
  let closed = false;
  let socket: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let catchup: Promise<void> | null = null;
  let eventQueue: Promise<void> = Promise.resolve();
  const lastSeq = () => handlers.getLastSeq?.() ?? 0;

  const fetchMissingEvents = () => {
    if (!catchup) {
      catchup = fetchWorkspaceEvents(workspaceId, lastSeq())
        .then((events) => {
          for (const event of events.sort((left, right) => left.seq - right.seq)) {
            if (event.seq > lastSeq()) {
              handlers.onEvent(event);
            }
          }
        })
        .finally(() => {
          catchup = null;
        });
    }
    return catchup;
  };

  const scheduleReconnect = () => {
    if (closed || reconnectTimer) return;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      void connect();
    }, RECONNECT_DELAY_MS);
  };

  const handleEvent = async (event: RunEventEnvelope) => {
    if (event.seq <= lastSeq()) return;
    if (event.seq > lastSeq() + 1) {
      try {
        await fetchMissingEvents();
      } catch {
        handlers.onStatus('offline');
        socket?.close();
        return;
      }
      if (event.seq <= lastSeq()) return;
      if (event.seq > lastSeq() + 1) {
        handlers.onStatus('offline');
        socket?.close();
        return;
      }
    }
    handlers.onEvent(event);
  };

  const openSocket = (ticket: string | null) => {
    const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
    const url = new URL(`${protocol}://${window.location.host}/ws`);
    url.searchParams.set('workspace_id', workspaceId);
    if (ticket) {
      url.searchParams.set('ticket', ticket);
    }
    socket = new WebSocket(url.toString());

    socket.addEventListener('open', () => handlers.onStatus('live'));
    socket.addEventListener('close', () => {
      handlers.onStatus('offline');
      scheduleReconnect();
    });
    socket.addEventListener('error', () => {
      handlers.onStatus('offline');
      socket?.close();
    });
    socket.addEventListener('message', (message) => {
      try {
        const parsed = RunEventEnvelopeSchema.safeParse(JSON.parse(String(message.data)));
        if (parsed.success) {
          if (parsed.data.ev === 'canvas.presence') {
            const presence = CanvasPresenceSchema.safeParse(parsed.data.data);
            if (presence.success) {
              handlers.onPresence?.(presence.data);
            }
            return;
          }
          eventQueue = eventQueue
            .then(() => handleEvent(parsed.data))
            .catch(() => {
              handlers.onStatus('offline');
              socket?.close();
            });
        } else {
          handlers.onStatus('offline');
          socket?.close();
        }
      } catch {
        handlers.onStatus('offline');
        socket?.close();
      }
    });
  };

  const connect = async () => {
    if (closed) return;
    handlers.onStatus('connecting');
    try {
      await fetchMissingEvents();
      if (closed) return;
      openSocket(await requestCanvasTicket(workspaceId));
    } catch {
      if (!closed) {
        handlers.onStatus('offline');
        scheduleReconnect();
      }
    }
  };

  if (typeof WebSocket === 'undefined') {
    void fetchMissingEvents().catch(() => undefined);
    handlers.onStatus('offline');
    return () => undefined;
  }

  void connect();

  return () => {
    closed = true;
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
    }
    socket?.close();
  };
}
