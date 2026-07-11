import type {
  ChatMessageKind,
  RunEventEnvelope,
  RunStatus,
  RunStepState,
  WorkbenchState,
} from './types';

export function applyRunEvent(state: WorkbenchState, event: RunEventEnvelope): WorkbenchState {
  if (event.workspace_id !== state.workspace.id) {
    return state;
  }

  if (isAgentStatusEvent(event.ev)) {
    return applyAgentStatusEvent(state, event);
  }

  if (!state.run || event.run_id !== state.run.id || event.seq <= state.eventSeq) {
    return state;
  }

  if (event.ev === 'node.state') {
    const nodeId = stringData(event, 'node_id');
    const nodeState = stepStateData(event, 'state');
    const cached = booleanData(event, 'cached');
    if (!nodeId || !nodeState) {
      return { ...state, eventSeq: event.seq };
    }

    return {
      ...state,
      eventSeq: event.seq,
      graph: {
        ...state.graph,
        nodes: state.graph.nodes.map((node) =>
          node.id === nodeId ? { ...node, status: nodeState, cached } : node,
        ),
      },
      run: {
        ...state.run,
        steps: state.run.steps.map((step) =>
          step.nodeId === nodeId
            ? { ...step, state: nodeState, cached, error: eventError(event) }
            : step,
        ),
      },
    };
  }

  if (event.ev === 'run.retry') {
    const childRunId = stringData(event, 'child_run_id');
    const attempt = numberData(event, 'attempt');
    const requiresConfirmation = booleanData(event, 'requires_confirmation');
    const messageId = `run-retry-${childRunId ?? event.seq}`;
    const message = {
      id: messageId,
      role: 'system' as const,
      kind: 'run_requested' as const,
      text: requiresConfirmation
        ? `Retry ${attempt ?? ''} is waiting for cost confirmation.`.replace('  ', ' ')
        : `Retry ${attempt ?? ''} started automatically.`.replace('  ', ' '),
      time: event.server_time,
    };
    return {
      ...state,
      eventSeq: event.seq,
      chat: {
        ...state.chat,
        messages: state.chat.messages.some((item) => item.id === messageId)
          ? state.chat.messages
          : [...state.chat.messages, message],
      },
    };
  }

  const nextRunStatus = runStatusFromEvent(event.ev);
  if (nextRunStatus) {
    return {
      ...state,
      eventSeq: event.seq,
      run: {
        ...state.run,
        status: nextRunStatus,
        error: nextRunStatus === 'failed' ? eventError(event) : state.run.error,
      },
    };
  }

  return { ...state, eventSeq: event.seq };
}

export function shouldRefetchWorkspaceState(
  state: WorkbenchState,
  event: RunEventEnvelope,
): boolean {
  return (
    event.workspace_id === state.workspace.id &&
    !isAgentStatusEvent(event.ev) &&
    (!state.run ||
      event.run_id !== state.run.id ||
      event.ev === 'run.retry' ||
      event.ev === 'run.retry_pending' ||
      event.ev === 'run.succeeded' ||
      event.ev === 'run.failed' ||
      event.ev === 'run.interrupted')
  );
}

function applyAgentStatusEvent(state: WorkbenchState, event: RunEventEnvelope): WorkbenchState {
  const sessionId = stringData(event, 'session_id') ?? event.run_id;
  const messageId = `agent-status-${sessionId}`;
  const text = agentStatusText(event);
  const message = {
    id: messageId,
    role: 'agent' as const,
    kind: agentMessageKind(event),
    text,
    time: event.server_time,
  };
  const found = state.chat.messages.some((item) => item.id === messageId);

  return {
    ...state,
    eventSeq: Math.max(state.eventSeq, event.seq),
    chat: {
      ...state.chat,
      messages: found
        ? state.chat.messages.map((item) => (item.id === messageId ? message : item))
        : [...state.chat.messages, message],
    },
  };
}

function isAgentStatusEvent(eventName: string): boolean {
  return eventName === 'agent.status' || eventName === 'agent.status.end' || eventName === 'agent.log';
}

function agentStatusText(event: RunEventEnvelope): string {
  const status = stringData(event, 'status') ?? event.ev;
  const detail = event.data.detail;
  if (detail && typeof detail === 'object' && !Array.isArray(detail)) {
    const record = detail as Record<string, unknown>;
    const message = record.message;
    if (typeof message === 'string' && message.length > 0) {
      return message;
    }
    const proposalTitle = record.proposal_title;
    if (typeof proposalTitle === 'string' && proposalTitle.length > 0) {
      return `Proposal ready: ${proposalTitle}`;
    }
  }
  return status;
}

function agentMessageKind(event: RunEventEnvelope): ChatMessageKind {
  const kind = stringData(event, 'message_kind');
  if (isChatMessageKind(kind)) {
    return kind;
  }
  const status = stringData(event, 'status');
  if (status === 'agent.status.end') {
    return 'chat';
  }
  return 'agent_log:status';
}

function isChatMessageKind(value: string | null): value is ChatMessageKind {
  if (typeof value !== 'string') {
    return false;
  }
  return (
    value === 'text' ||
    value === 'chat' ||
    value === 'proposal_pending' ||
    value === 'proposal_applied' ||
    value === 'proposal_dismissed' ||
    value === 'run_requested' ||
    value === 'run_failed' ||
    value.startsWith('agent_log:')
  );
}

function stringData(event: RunEventEnvelope, key: string): string | null {
  const value = event.data[key];
  return typeof value === 'string' ? value : null;
}

function booleanData(event: RunEventEnvelope, key: string): boolean {
  const value = event.data[key];
  return typeof value === 'boolean' ? value : false;
}

function numberData(event: RunEventEnvelope, key: string): number | null {
  const value = event.data[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function stepStateData(event: RunEventEnvelope, key: string): RunStepState | null {
  const value = event.data[key];
  if (
    value === 'queued' ||
    value === 'running' ||
    value === 'succeeded' ||
    value === 'failed' ||
    value === 'skipped'
  ) {
    return value;
  }
  return null;
}

function eventError(event: RunEventEnvelope): { summary: string; raw?: string | null } | null {
  const error = stringData(event, 'error');
  if (!error) {
    return null;
  }
  return {
    summary: firstLine(error).slice(0, 160),
    raw: error.slice(0, 1200),
  };
}

function firstLine(value: string): string {
  return value.split(/\r?\n/, 1)[0] ?? value;
}

function runStatusFromEvent(eventName: string): RunStatus | null {
  if (eventName === 'run.started') {
    return 'running';
  }
  if (eventName === 'run.succeeded') {
    return 'succeeded';
  }
  if (eventName === 'run.failed') {
    return 'failed';
  }
  if (eventName === 'run.interrupted') {
    return 'interrupted';
  }
  return null;
}
