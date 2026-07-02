import {
  RunConfirmationResponseSchema,
  RunEventEnvelopeSchema,
  WorkflowGraphSchema,
  WorkspaceSummarySchema,
  WorkspaceMessageResponseSchema,
  WorkbenchStateSchema,
  NodeCatalogSchema,
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

export async function queueWorkspaceRun(workspaceId: string): Promise<RunConfirmationResponse> {
  const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/runs`, {
    method: 'POST',
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
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/proposals/manual`,
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
        : `manual proposal request failed: ${response.status}`;
    throw new Error(message);
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
