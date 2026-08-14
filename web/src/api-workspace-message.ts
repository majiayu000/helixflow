import {
  WorkspaceMessageResponseSchema,
  type CanvasMessageContext,
  type Conversation,
  type TurnMode,
  type WorkflowGraph,
  type WorkspaceMessageResponse,
} from './types';

export async function sendWorkspaceMessage(
  workspaceId: string,
  input: {
    baseVersionId: string;
    userMessage: string;
    graph: WorkflowGraph;
    canvasContext?: CanvasMessageContext;
    conversationId?: string;
    turnMode?: TurnMode;
  },
  signal?: AbortSignal,
): Promise<WorkspaceMessageResponse> {
  const response = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/messages`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(input),
    signal,
  });
  const body = await response.json();
  if (!response.ok) {
    const message =
      body && typeof body === 'object' && 'error' in body && typeof body.error === 'string'
        ? body.error
        : `workspace message request failed: ${response.status}`;
    throw new Error(message);
  }

  const parsed = WorkspaceMessageResponseSchema.parse(body);
  const terminalKinds = {
    succeeded: new Set([
      'chat',
      'proposal_pending',
      'proposal_applied',
      'run_requested',
      'agent_status',
    ]),
    clarify: new Set(['clarify']),
    error: new Set(['agent_error', 'run_failed']),
    interrupted: new Set(['agent_interrupted']),
  } as const;
  const terminalStatus =
    parsed.turnStatus && parsed.turnStatus !== 'running' ? parsed.turnStatus : null;
  const hasMatchingTerminalMessage =
    parsed.turnId &&
    terminalStatus &&
    parsed.messages.some(
      (message) =>
        message.role === 'agent' &&
        message.turnId === parsed.turnId &&
        Boolean(message.kind && terminalKinds[terminalStatus].has(message.kind)),
    );
  if (
    !parsed.turnId ||
    !parsed.turnStatus ||
    parsed.turnStatus === 'running' ||
    !hasMatchingTerminalMessage
  ) {
    throw new Error('workspace message response is missing a durable terminal turn');
  }
  return parsed;
}

export async function createWorkspaceConversation(
  workspaceId: string,
  title = '新对话',
  signal?: AbortSignal,
): Promise<Conversation> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/conversations`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ title }),
      signal,
    },
  );
  const body = await response.json();
  if (!response.ok) {
    throw new Error(
      body && typeof body.error === 'string'
        ? body.error
        : `conversation create request failed: ${response.status}`,
    );
  }
  return {
    id: String(body.id),
    title: String(body.title),
    codexThreadId: typeof body.codexThreadId === 'string' ? body.codexThreadId : null,
    createdAt: String(body.createdAt),
    updatedAt: String(body.updatedAt),
  };
}
