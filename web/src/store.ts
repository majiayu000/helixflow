import { create } from 'zustand';
import {
  applyWorkspaceProposal,
  confirmWorkspaceRun,
  createManualWorkspaceProposal,
  createWorkspace,
  dismissWorkspaceProposal,
  exportWorkflowVersion,
  fetchWorkspaceState,
  fetchWorkspaces,
  holdWorkspaceRun,
  interruptRun as interruptRunRequest,
  queueWorkspaceRun,
  restoreWorkspaceVersion,
  saveWorkspaceLayout,
  selectOutput as selectOutputRequest,
  selectWorkspaceProvider,
  sendWorkspaceMessage,
  undoWorkspaceVersion,
  type ConnectionStatus,
} from './api';
import type {
  ChatMessageKind,
  CanvasMessageContext,
  LayoutPositionUpdate,
  ManualProposalInput,
  RunConfirmationResponse,
  RunEventEnvelope,
  RunStatus,
  RunStepState,
  WorkflowGraph,
  WorkspaceMessageResponse,
  WorkbenchState,
} from './types';

type LoadStatus = 'idle' | 'loading' | 'ready' | 'error';

type WorkbenchStore = {
  status: LoadStatus;
  error: string | null;
  connection: ConnectionStatus;
  state: WorkbenchState | null;
  bootstrap: (workspaceId?: string | null) => Promise<void>;
  hydrate: (workspaceId: string) => Promise<void>;
  createWorkspace: () => Promise<void>;
  setInitialState: (state: WorkbenchState) => void;
  setConnection: (status: ConnectionStatus) => void;
  applyEvent: (event: RunEventEnvelope) => void;
  sendMessage: (text: string, canvasContext?: CanvasMessageContext) => Promise<void>;
  queueRun: () => Promise<void>;
  interruptRun: (runId?: string) => Promise<void>;
  exportWorkflow: () => Promise<WorkflowGraph | null>;
  undoVersion: () => Promise<void>;
  restoreVersion: (versionId: string) => Promise<void>;
  saveLayout: (positions: LayoutPositionUpdate[]) => Promise<void>;
  selectOutput: (outputId: string) => Promise<void>;
  selectProvider: (providerId: string) => Promise<void>;
  confirmRun: (runId: string) => Promise<void>;
  holdRun: (runId: string) => Promise<void>;
  createManualProposal: (input: ManualProposalInput) => Promise<void>;
  applyProposal: (proposalId: string) => Promise<void>;
  dismissProposal: (proposalId: string) => Promise<void>;
};

export const useWorkbenchStore = create<WorkbenchStore>((set, get) => {
  let snapshotRefresh: Promise<void> | null = null;
  const refreshSnapshot = (workspaceId: string) => {
    if (snapshotRefresh) {
      return;
    }
    snapshotRefresh = fetchWorkspaceState(workspaceId)
      .then((state) =>
        set((current) =>
          current.state?.workspace.id === workspaceId
            ? { status: 'ready', state, error: null }
            : {},
        ),
      )
      .catch((error) => {
        const message =
          error instanceof Error ? error.message : 'workspace state request failed';
        set((current) =>
          current.state?.workspace.id === workspaceId ? { error: message } : {},
        );
      })
      .finally(() => {
        snapshotRefresh = null;
      });
  };

  return {
  status: 'idle',
  error: null,
  connection: 'offline',
  state: null,
  bootstrap: async (workspaceId) => {
    if (workspaceId) {
      await get().hydrate(workspaceId);
      return;
    }

    set({ status: 'loading', error: null });
    try {
      const workspaces = await fetchWorkspaces();
      const workspace = workspaces[0] ?? (await createWorkspace());
      const state = await fetchWorkspaceState(workspace.id);
      set({ status: 'ready', state, error: null });
    } catch (error) {
      set({
        status: 'error',
        error: error instanceof Error ? error.message : 'workspace bootstrap request failed',
      });
    }
  },
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
  createWorkspace: async () => {
    set({ status: 'loading', error: null });
    try {
      const workspace = await createWorkspace();
      const state = await fetchWorkspaceState(workspace.id);
      set({ status: 'ready', state, error: null });
    } catch (error) {
      set({
        status: 'error',
        error: error instanceof Error ? error.message : 'workspace create request failed',
      });
    }
  },
  setInitialState: (state) => set({ status: 'ready', state, error: null }),
  setConnection: (connection) => {
    set({ connection });
    const state = get().state;
    if (connection === 'live' && state) {
      refreshSnapshot(state.workspace.id);
    }
  },
  sendMessage: async (text, canvasContext) => {
    const trimmed = text.trim();
    if (!trimmed) {
      return;
    }

    const state = get().state;
    if (!state) {
      return;
    }
    const request = {
      workspaceId: state.workspace.id,
      baseVersionId: state.workspace.versionId,
      graph: workflowGraphFromState(state),
    };

    set({
      state: appendChatMessages(state, [
        {
          id: `msg_user_${Date.now()}`,
          role: 'user',
          kind: 'text',
          text: trimmed,
          time: messageTime(),
        },
      ]),
    });

    try {
      const response = await sendWorkspaceMessage(request.workspaceId, {
        baseVersionId: request.baseVersionId,
        canvasContext,
        userMessage: trimmed,
        graph: request.graph,
      });
      set((current) => ({
        state: current.state ? applyMessageResponse(current.state, response) : current.state,
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'workspace message request failed';
      set((current) => ({
        state: current.state
          ? appendChatMessages(current.state, [
              {
                id: `msg_system_${Date.now()}`,
                role: 'system',
                kind: 'run_failed',
                text: message,
                time: messageTime(),
              },
            ])
          : current.state,
      }));
    }
  },
  applyEvent: (event) => {
    const state = get().state;
    if (!state) {
      return;
    }
    const needsSnapshot = shouldRefetchWorkspaceState(state, event);
    set({ state: applyRunEvent(state, event) });
    if (needsSnapshot) {
      refreshSnapshot(state.workspace.id);
    }
  },
  queueRun: async () => {
    const state = get().state;
    if (!state) {
      return;
    }

    try {
      const response = await queueWorkspaceRun(state.workspace.id);
      set((current) => ({
        state: current.state ? applyRunConfirmation(current.state, response) : current.state,
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'run queue request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  interruptRun: async (runId) => {
    const state = get().state;
    const targetRunId = runId ?? state?.run?.id;
    if (!state || !targetRunId) {
      return;
    }

    try {
      const response = await interruptRunRequest(targetRunId);
      set((current) => ({
        state: current.state ? applyRunConfirmation(current.state, response) : current.state,
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'run interrupt request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  exportWorkflow: async () => {
    const state = get().state;
    const versionId = state?.workspace.versionId;
    if (!state || !versionId) {
      return null;
    }

    try {
      return await exportWorkflowVersion(versionId);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'workflow export request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
      return null;
    }
  },
  undoVersion: async () => {
    const state = get().state;
    if (!state) {
      return;
    }

    try {
      const next = await undoWorkspaceVersion(state.workspace.id);
      set({ state: next, status: 'ready', error: null });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'workspace undo request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  restoreVersion: async (versionId) => {
    const state = get().state;
    if (!state) {
      return;
    }

    try {
      const next = await restoreWorkspaceVersion(state.workspace.id, versionId);
      set({ state: next, status: 'ready', error: null });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'workspace restore request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  saveLayout: async (positions) => {
    const state = get().state;
    if (!state || positions.length === 0) {
      return;
    }

    try {
      const next = await saveWorkspaceLayout(state.workspace.id, {
        baseVersionId: state.workspace.versionId,
        positions,
      });
      set({ state: next, status: 'ready', error: null });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'workspace layout request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  selectOutput: async (outputId) => {
    const state = get().state;
    if (!state) {
      return;
    }

    try {
      const next = await selectOutputRequest(outputId);
      set({ state: next, status: 'ready', error: null });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'output select request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  selectProvider: async (providerId) => {
    const state = get().state;
    if (!state) {
      return;
    }

    try {
      const next = await selectWorkspaceProvider(state.workspace.id, providerId);
      set({ state: next, status: 'ready', error: null });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'workspace provider request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  confirmRun: async (runId) => {
    const state = get().state;
    if (!state) {
      return;
    }
    const previousRun = state.run;
    const previousPendingConfirmation = state.pendingConfirmation;

    set((current) => ({
      state: current.state ? markRunConfirming(current.state, runId) : current.state,
    }));
    try {
      const response = await confirmWorkspaceRun(state.workspace.id, runId);
      set((current) => ({
        state: current.state ? applyRunConfirmation(current.state, response) : current.state,
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'run confirmation request failed';
      set((current) => ({
        state:
          current.state && current.state.workspace.id === state.workspace.id
            ? appendSystemError(
                {
                  ...current.state,
                  run: previousRun,
                  pendingConfirmation: previousPendingConfirmation,
                },
                message,
              )
            : current.state,
      }));
    }
  },
  holdRun: async (runId) => {
    const state = get().state;
    if (!state) {
      return;
    }

    try {
      const response = await holdWorkspaceRun(state.workspace.id, runId);
      set((current) => ({
        state: current.state ? applyRunConfirmation(current.state, response) : current.state,
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'run hold request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  createManualProposal: async (input) => {
    const state = get().state;
    if (!state) {
      return;
    }

    try {
      const next = await createManualWorkspaceProposal(state.workspace.id, input);
      set({ state: next, status: 'ready', error: null });
    } catch (error) {
      const normalized =
        error instanceof Error ? error : new Error('manual edit request failed');
      const message = normalized.message;
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
      throw normalized;
    }
  },
  applyProposal: async (proposalId) => {
    const state = get().state;
    if (!state) {
      return;
    }

    try {
      const next = await applyWorkspaceProposal(state.workspace.id, proposalId);
      set({ state: next, status: 'ready', error: null });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'proposal apply request failed';
      set((current) => ({
        state: current.state
          ? appendChatMessages(current.state, [
              {
                id: `msg_system_${Date.now()}`,
                role: 'system',
                kind: 'run_failed',
                text: message,
                time: messageTime(),
              },
            ])
          : current.state,
      }));
    }
  },
  dismissProposal: async (proposalId) => {
    const state = get().state;
    if (!state) {
      return;
    }

    try {
      const next = await dismissWorkspaceProposal(state.workspace.id, proposalId);
      set({ state: next, status: 'ready', error: null });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'proposal dismiss request failed';
      set((current) => ({
        state: current.state
          ? appendChatMessages(current.state, [
              {
                id: `msg_system_${Date.now()}`,
                role: 'system',
                kind: 'run_failed',
                text: message,
                time: messageTime(),
              },
            ])
          : current.state,
      }));
    }
  },
  };
});

function appendChatMessages(
  state: WorkbenchState,
  messages: WorkbenchState['chat']['messages'],
): WorkbenchState {
  return {
    ...state,
    chat: {
      ...state.chat,
      messages: [...state.chat.messages, ...messages],
    },
  };
}

function appendSystemError(state: WorkbenchState, message: string): WorkbenchState {
  return appendChatMessages(state, [
    {
      id: `msg_system_${Date.now()}`,
      role: 'system',
      kind: 'run_failed',
      text: message,
      time: messageTime(),
    },
  ]);
}

function applyMessageResponse(
  state: WorkbenchState,
  response: WorkspaceMessageResponse,
): WorkbenchState {
  let next = appendChatMessages(state, response.messages);
  if (response.run) {
    next = applyRunSnapshot(next, response.run);
  }
  if (response.proposal) {
    next = {
      ...next,
      pendingProposal: response.proposal,
    };
  }
  if (response.pendingConfirmation !== undefined) {
    next = {
      ...next,
      pendingConfirmation: response.pendingConfirmation,
    };
  }
  return next;
}

function applyRunConfirmation(
  state: WorkbenchState,
  response: RunConfirmationResponse,
): WorkbenchState {
  return applyRunSnapshot(
    {
      ...state,
      outputs: response.outputs,
      pendingConfirmation: response.pendingConfirmation,
    },
    response.run,
  );
}

function markRunConfirming(state: WorkbenchState, runId: string): WorkbenchState {
  return {
    ...state,
    pendingConfirmation: null,
    run:
      state.run?.id === runId
        ? {
            ...state.run,
            status: 'running',
          }
        : state.run,
  };
}

function applyRunSnapshot(
  state: WorkbenchState,
  run: NonNullable<WorkbenchState['run']>,
): WorkbenchState {
  const eventSeq = state.run?.id === run.id ? state.eventSeq : 0;

  return {
    ...state,
    eventSeq,
    run,
    graph: {
      ...state.graph,
      nodes: state.graph.nodes.map((node) => {
        const step = run.steps.find((candidate) => candidate.nodeId === node.id);
        return step ? { ...node, status: step.state, provider: step.provider } : node;
      }),
    },
  };
}

function workflowGraphFromState(state: WorkbenchState): WorkflowGraph {
  if (state.workflowGraph) {
    return state.workflowGraph;
  }

  return {
    schema_version: 1,
    nodes: Object.fromEntries(
      state.graph.nodes.map((node) => [
        node.id,
        {
          node_type: node.nodeType,
          title: node.title,
          params: {},
          pos: [node.position.x, node.position.y] as [number, number],
        },
      ]),
    ),
    edges: state.graph.edges.map((edge) => ({
      from: [edge.from.nodeId, edge.from.port],
      to: [edge.to.nodeId, edge.to.port],
      edge_type: edge.kind,
    })),
  };
}

function messageTime(): string {
  return new Date().toISOString();
}

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
          step.nodeId === nodeId ? { ...step, state: nodeState, error: eventError(event) } : step,
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

function shouldRefetchWorkspaceState(state: WorkbenchState, event: RunEventEnvelope): boolean {
  return (
    event.workspace_id === state.workspace.id &&
    !isAgentStatusEvent(event.ev) &&
    (!state.run ||
      event.run_id !== state.run.id ||
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
