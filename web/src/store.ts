import { create } from 'zustand';
import { fetchWorkspaceState, type ConnectionStatus } from './api';
import type { RunEventEnvelope, RunStatus, RunStepState, WorkbenchState } from './types';

type LoadStatus = 'idle' | 'loading' | 'ready' | 'error';

type WorkbenchStore = {
  status: LoadStatus;
  error: string | null;
  connection: ConnectionStatus;
  state: WorkbenchState | null;
  hydrate: (workspaceId: string) => Promise<void>;
  setInitialState: (state: WorkbenchState) => void;
  setConnection: (status: ConnectionStatus) => void;
  applyEvent: (event: RunEventEnvelope) => void;
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
}));

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
  const sessionId = stringData(event, 'session_id') ?? event.run_id;
  const messageId = `agent-status-${sessionId}`;
  const text = agentStatusText(event);
  const message = {
    id: messageId,
    role: 'agent' as const,
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
