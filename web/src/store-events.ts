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

  if (
    event.ev === 'run.retry' ||
    event.ev === 'run.retry_pending' ||
    event.ev === 'run.retry_failed'
  ) {
    return applyRetryNotice(state, event);
  }

  if (
    event.ev === 'run.fix_attempt' ||
    event.ev === 'run.fix_applied' ||
    event.ev === 'run.fix_exhausted'
  ) {
    return applyRunFixNotice(state, event);
  }

  if (event.ev === 'run.remote_cancel_unsupported' || event.ev === 'run.remote_cancel_failed') {
    return applyRemoteCancelNotice(state, event);
  }

  if (isRecoveryEvent(event.ev)) {
    return applyRecoveryNotice(state, event);
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

export function preserveRetryNotices(
  current: WorkbenchState,
  snapshot: WorkbenchState,
): WorkbenchState {
  const existingIds = new Set(snapshot.chat.messages.map((message) => message.id));
  const notices = current.chat.messages.filter(
      (message) =>
      (message.id.startsWith('run-retry-') ||
        message.id.startsWith('run-remote-cancel-') ||
        message.id.startsWith('run-recovery-') ||
        message.id.startsWith('run-fix-')) &&
      !existingIds.has(message.id),
  );
  if (notices.length === 0) {
    return snapshot;
  }
  return {
    ...snapshot,
    chat: {
      ...snapshot.chat,
      messages: [...snapshot.chat.messages, ...notices],
    },
  };
}

function applyRunFixNotice(state: WorkbenchState, event: RunEventEnvelope): WorkbenchState {
  const attempt = numberData(event, 'attempt');
  const maxAttempts = numberData(event, 'max_attempts');
  const requiresConfirmation = booleanData(event, 'requires_confirmation');
  const reasonCode = stringData(event, 'reason_code') ?? 'FIX_EXHAUSTED';
  const operationId = stringData(event, 'operation_id') ?? event.run_id;
  const messageId = `run-fix-${event.ev}-${operationId}-${event.seq}`;
  let text: string;
  if (event.ev === 'run.fix_attempt') {
    text = `Agent workflow repair attempt ${attempt ?? '?'} of ${maxAttempts ?? '?'} started.`;
  } else if (event.ev === 'run.fix_applied') {
    text = requiresConfirmation
      ? 'The workflow was repaired as a new version. Its provider run is waiting for cost confirmation.'
      : 'The workflow was repaired as a new version. Its provider run started automatically; execution has not succeeded yet.';
  } else {
    text = `Automatic workflow repair stopped (${reasonCode}). Manual review is required.`;
  }
  if (state.chat.messages.some((message) => message.id === messageId)) {
    return state;
  }
  return {
    ...state,
    eventSeq: Math.max(state.eventSeq, event.seq),
    chat: {
      ...state.chat,
      messages: [
        ...state.chat.messages,
        {
          id: messageId,
          role: 'system',
          kind: event.ev === 'run.fix_exhausted' ? 'run_failed' : 'run_requested',
          text,
          time: event.server_time,
        },
      ],
    },
  };
}

function isRecoveryEvent(eventName: string): boolean {
  return (
    eventName === 'run.recovery_started' ||
    eventName === 'run.recovery_succeeded' ||
    eventName === 'run.recovery_cancelled' ||
    eventName === 'run.recovery_abandoned'
  );
}

function applyRecoveryNotice(state: WorkbenchState, event: RunEventEnvelope): WorkbenchState {
  const provider = stringData(event, 'provider') ?? '远端 provider';
  const messageId = `run-recovery-${event.ev}-${event.run_id}-${event.seq}`;
  const text =
    event.ev === 'run.recovery_started'
      ? '服务重启后正在恢复此运行。'
      : event.ev === 'run.recovery_succeeded'
        ? '服务重启后的运行恢复已完成。'
        : event.ev === 'run.recovery_cancelled'
          ? `恢复期间已取消 ${provider} 的远端任务。`
          : (stringData(event, 'message') ??
            `${provider} 的远端任务终态无法确认，可能继续产生费用。`);
  if (state.chat.messages.some((item) => item.id === messageId)) {
    return state;
  }
  return {
    ...state,
    eventSeq: Math.max(state.eventSeq, event.seq),
    chat: {
      ...state.chat,
      messages: [
        ...state.chat.messages,
        {
          id: messageId,
          role: 'system',
          kind: event.ev === 'run.recovery_abandoned' ? 'run_failed' : 'run_requested',
          text,
          time: event.server_time,
        },
      ],
    },
  };
}

function applyRemoteCancelNotice(state: WorkbenchState, event: RunEventEnvelope): WorkbenchState {
  const provider = stringData(event, 'provider') ?? 'remote provider';
  const messageId = `run-remote-cancel-${event.ev}-${event.run_id}-${event.seq}`;
  const text =
    event.ev === 'run.remote_cancel_unsupported'
      ? (stringData(event, 'message') ??
        `Provider \`${provider}\` cannot cancel already-submitted remote tasks; the remote task may keep running and incur charges.`)
      : `Failed to cancel the remote ${provider} task: ${stringData(event, 'error') ?? 'unknown error'}. It may keep running and incur charges.`;
  if (state.chat.messages.some((item) => item.id === messageId)) {
    return state;
  }
  return {
    ...state,
    eventSeq: Math.max(state.eventSeq, event.seq),
    chat: {
      ...state.chat,
      messages: [
        ...state.chat.messages,
        {
          id: messageId,
          role: 'system',
          kind: 'run_failed',
          text,
          time: event.server_time,
        },
      ],
    },
  };
}

function applyRetryNotice(state: WorkbenchState, event: RunEventEnvelope): WorkbenchState {
  const childRunId = stringData(event, 'child_run_id') ?? event.run_id;
  const attempt = numberData(event, 'attempt');
  const requiresConfirmation = booleanData(event, 'requires_confirmation');
  const messageId = `run-retry-${event.ev}-${childRunId}-${event.seq}`;
  let text: string;
  if (event.ev === 'run.retry_failed') {
    text = `Retry failed: ${stringData(event, 'error') ?? 'unknown retry error'}`;
  } else if (event.ev === 'run.retry_pending' || requiresConfirmation) {
    text = `Retry ${attempt ?? ''} is waiting for cost confirmation.`.replace('  ', ' ');
  } else {
    text = `Retry ${attempt ?? ''} started automatically.`.replace('  ', ' ');
  }
  if (state.chat.messages.some((item) => item.id === messageId)) {
    return state;
  }
  return {
    ...state,
    eventSeq: Math.max(state.eventSeq, event.seq),
    chat: {
      ...state.chat,
      messages: [
        ...state.chat.messages,
        {
          id: messageId,
          role: 'system',
          kind: event.ev === 'run.retry_failed' ? 'run_failed' : 'run_requested',
          text,
          time: event.server_time,
        },
      ],
    },
  };
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
      event.ev === 'run.retry_failed' ||
      event.ev === 'run.fix_applied' ||
      event.ev === 'run.fix_exhausted' ||
      isRecoveryEvent(event.ev) ||
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
