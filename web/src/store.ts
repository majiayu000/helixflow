import { create } from 'zustand';
import {
  applyWorkspaceProposal,
  approveWorkspaceConfirmation,
  dismissWorkspaceProposal,
  fetchWorkspaceState,
  holdWorkspaceConfirmation,
  postWorkspaceMessage,
  requestWorkspaceRun,
  type ConnectionStatus,
} from './api';
import type { RunEventEnvelope, RunStatus, RunStepState, WorkbenchState } from './types';

type LoadStatus = 'idle' | 'loading' | 'ready' | 'error';
type ChatMessage = WorkbenchState['chat']['messages'][number];

type WorkbenchStore = {
  status: LoadStatus;
  error: string | null;
  connection: ConnectionStatus;
  state: WorkbenchState | null;
  hydrate: (workspaceId: string) => Promise<void>;
  setInitialState: (state: WorkbenchState) => void;
  setConnection: (status: ConnectionStatus) => void;
  applyEvent: (event: RunEventEnvelope) => void;
  sendMessage: (workspaceId: string, text: string) => Promise<void>;
  requestRun: (workspaceId: string) => Promise<void>;
  applyProposal: (workspaceId: string, proposalId: string) => Promise<void>;
  dismissProposal: (workspaceId: string, proposalId: string) => Promise<void>;
  approveConfirmation: (workspaceId: string, confirmationId: string) => Promise<void>;
  holdConfirmation: (workspaceId: string, confirmationId: string) => Promise<void>;
};

export const useWorkbenchStore = create<WorkbenchStore>((set) => ({
  status: 'idle',
  error: null,
  connection: 'offline',
  state: null,
  hydrate: async (workspaceId) => {
    set({ status: 'loading', error: null });
    try {
      const state = await fetchWorkspaceState(workspaceId);
      set({ status: 'ready', state, error: null });
    } catch (error) {
      set({
        status: 'error',
        error: error instanceof Error ? error.message : 'workspace state request failed',
      });
    }
  },
  setInitialState: (state) => set({ status: 'ready', state, error: null }),
  setConnection: (connection) => set({ connection }),
  applyEvent: (event) =>
    set((current) => ({
      state: current.state ? applyRunEvent(current.state, event) : current.state,
    })),
  sendMessage: async (workspaceId, text) => {
    appendOptimisticUserMessage(set, workspaceId, text);
    await applyWorkspaceAction(set, () => postWorkspaceMessage(workspaceId, text));
  },
  requestRun: async (workspaceId) => {
    await applyWorkspaceAction(set, () => requestWorkspaceRun(workspaceId));
  },
  applyProposal: async (workspaceId, proposalId) => {
    await applyWorkspaceAction(set, () => applyWorkspaceProposal(workspaceId, proposalId));
  },
  dismissProposal: async (workspaceId, proposalId) => {
    await applyWorkspaceAction(set, () => dismissWorkspaceProposal(workspaceId, proposalId));
  },
  approveConfirmation: async (workspaceId, confirmationId) => {
    await applyWorkspaceAction(set, () =>
      approveWorkspaceConfirmation(workspaceId, confirmationId),
    );
  },
  holdConfirmation: async (workspaceId, confirmationId) => {
    await applyWorkspaceAction(set, () => holdWorkspaceConfirmation(workspaceId, confirmationId));
  },
}));

async function applyWorkspaceAction(
  set: (
    partial:
      | Partial<WorkbenchStore>
      | ((current: WorkbenchStore) => Partial<WorkbenchStore>),
  ) => void,
  action: () => Promise<WorkbenchState>,
): Promise<void> {
  set({ error: null });
  try {
    const state = await action();
    set((current) => ({
      status: 'ready',
      state: current.state ? mergeTransientMessages(current.state, state) : state,
      error: null,
    }));
  } catch (error) {
    set({
      status: 'error',
      error: error instanceof Error ? error.message : 'workspace action failed',
    });
  }
}

export function applyRunEvent(state: WorkbenchState, event: RunEventEnvelope): WorkbenchState {
  if (event.workspace_id !== state.workspace.id) {
    return state;
  }

  if (isAgentStatusEvent(event.ev)) {
    return applyAgentStatusEvent(state, event);
  }

  if (event.run_id !== state.run.id || event.seq <= state.eventSeq) {
    return state;
  }

  if (event.ev === 'node.state') {
    const nodeId = stringData(event, 'node_id');
    const nodeState = stepStateData(event, 'state');
    if (!nodeId || !nodeState) {
      return { ...state, eventSeq: event.seq };
    }

    return {
      ...state,
      eventSeq: event.seq,
      graph: {
        ...state.graph,
        nodes: state.graph.nodes.map((node) =>
          node.id === nodeId ? { ...node, status: nodeState } : node,
        ),
      },
      run: {
        ...state.run,
        steps: state.run.steps.map((step) =>
          step.nodeId === nodeId ? { ...step, state: nodeState } : step,
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
      },
    };
  }

  return { ...state, eventSeq: event.seq };
}

function applyAgentStatusEvent(state: WorkbenchState, event: RunEventEnvelope): WorkbenchState {
  const status = stringData(event, 'status') ?? event.ev;
  const sessionId = stringData(event, 'session_id') ?? event.run_id;
  const messageId = `agent-status-${sessionId}`;

  if (status === 'runtime.log') {
    return appendAgentLogEvent(state, event);
  }
  if (status === 'agent.status.end' || event.ev === 'agent.status.end') {
    return {
      ...state,
      eventSeq: Math.max(state.eventSeq, event.seq),
      chat: {
        ...state.chat,
        messages: state.chat.messages.filter((item) => item.id !== messageId),
      },
    };
  }
  if (hasFinalAgentMessage(state.chat.messages, sessionId)) {
    return { ...state, eventSeq: Math.max(state.eventSeq, event.seq) };
  }

  const text = agentStatusText(event);
  const message = {
    id: messageId,
    role: 'agent' as const,
    text,
    time: event.server_time,
    kind: 'agent_status',
    label: status,
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

function appendAgentLogEvent(state: WorkbenchState, event: RunEventEnvelope): WorkbenchState {
  const sessionId = stringData(event, 'session_id') ?? event.run_id;
  const detail = detailRecord(event);
  const kind = typeof detail.kind === 'string' ? detail.kind : 'runtime';
  const label = typeof detail.label === 'string' ? detail.label : kind;
  const text = typeof detail.text === 'string' ? detail.text : JSON.stringify(detail);
  const raw = typeof detail.raw === 'string' ? detail.raw : undefined;
  const message = {
    id: `agent-log-${sessionId}-${event.seq}`,
    role: 'agent' as const,
    text,
    time: event.server_time,
    kind: `agent_log:${kind}`,
    label,
    raw,
  };
  if (
    state.chat.messages.some(
      (item) => item.id === message.id || isEquivalentAgentLog(item, message),
    )
  ) {
    return { ...state, eventSeq: Math.max(state.eventSeq, event.seq) };
  }
  return {
    ...state,
    eventSeq: Math.max(state.eventSeq, event.seq),
    chat: {
      ...state.chat,
      messages: [...state.chat.messages, message],
    },
  };
}

function appendOptimisticUserMessage(
  set: (
    partial:
      | Partial<WorkbenchStore>
      | ((current: WorkbenchStore) => Partial<WorkbenchStore>),
  ) => void,
  workspaceId: string,
  text: string,
) {
  const trimmed = text.trim();
  if (!trimmed) return;
  set((current) => {
    if (!current.state || current.state.workspace.id !== workspaceId) {
      return { error: null };
    }
    const message: ChatMessage = {
      id: `optimistic-user-${Date.now()}`,
      role: 'user',
      text: trimmed,
      time: new Date().toISOString(),
      kind: 'text',
      label: null,
    };
    return {
      error: null,
      state: {
        ...current.state,
        chat: {
          ...current.state.chat,
          messages: [...current.state.chat.messages, message],
        },
      },
    };
  });
}

function mergeTransientMessages(current: WorkbenchState, next: WorkbenchState): WorkbenchState {
  const nextMessages = dedupeChatMessages(next.chat.messages);
  const transient = current.chat.messages.filter((message) =>
    shouldCarryTransientMessage(message, nextMessages),
  );
  return {
    ...next,
    chat: {
      ...next.chat,
      messages: dedupeChatMessages([...nextMessages, ...transient]),
    },
  };
}

function shouldCarryTransientMessage(message: ChatMessage, nextMessages: ChatMessage[]): boolean {
  if (isOptimisticUserMessage(message)) {
    return !nextMessages.some(
      (item) => item.role === 'user' && item.text.trim() === message.text.trim(),
    );
  }
  if (isTransientAgentLog(message)) {
    return !nextMessages.some((item) => isEquivalentAgentLog(item, message));
  }
  return false;
}

function dedupeChatMessages(messages: ChatMessage[]): ChatMessage[] {
  const seenIds = new Set<string>();
  const seenAgentLogs = new Set<string>();
  const next: ChatMessage[] = [];
  for (const message of messages) {
    if (seenIds.has(message.id)) continue;
    seenIds.add(message.id);
    if (isAgentLogMessage(message)) {
      const signature = agentLogSignature(message);
      if (seenAgentLogs.has(signature)) continue;
      seenAgentLogs.add(signature);
    }
    next.push(message);
  }
  return next;
}

function isOptimisticUserMessage(message: ChatMessage): boolean {
  return message.role === 'user' && message.id.startsWith('optimistic-user-');
}

function isTransientAgentLog(message: ChatMessage): boolean {
  return message.id.startsWith('agent-log-') && isAgentLogMessage(message);
}

function isAgentLogMessage(message: ChatMessage): boolean {
  return message.kind?.startsWith('agent_log:') ?? false;
}

function isEquivalentAgentLog(left: ChatMessage, right: ChatMessage): boolean {
  return isAgentLogMessage(left) && isAgentLogMessage(right) && agentLogSignature(left) === agentLogSignature(right);
}

function agentLogSignature(message: ChatMessage): string {
  return [message.kind ?? '', message.label ?? '', message.text.trim()].join('\u0000');
}

function hasFinalAgentMessage(messages: ChatMessage[], sessionId: string): boolean {
  return messages.some(
    (message) =>
      message.role === 'agent' && message.kind === 'chat' && message.label === sessionId,
  );
}

function detailRecord(event: RunEventEnvelope): Record<string, unknown> {
  const detail = event.data.detail;
  return detail && typeof detail === 'object' && !Array.isArray(detail)
    ? (detail as Record<string, unknown>)
    : {};
}

function isAgentStatusEvent(eventName: string): boolean {
  return eventName === 'agent.status' || eventName === 'agent.status.end';
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
  return statusLabel(status);
}

function statusLabel(status: string): string {
  if (status === 'ctx.created') return '正在读取工作区上下文';
  if (status === 'runtime.started') return 'Codex CLI 已启动';
  if (status === 'turn.sent') return 'Agent 正在处理请求';
  if (status === 'agent.status.end') return 'Agent 已完成';
  return status;
}

function stringData(event: RunEventEnvelope, key: string): string | null {
  const value = event.data[key];
  return typeof value === 'string' ? value : null;
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
