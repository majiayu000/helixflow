import { create } from 'zustand';
import {
  applyWorkspaceProposal,
  confirmWorkspaceRun,
  createManualWorkspaceProposal,
  createWorkspace,
  dismissWorkspaceProposal,
  exportWorkflowVersion,
  fetchWorkspaceCanvas,
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
  CanvasDocument,
  CanvasMessageContext,
  LayoutPositionUpdate,
  ManualEditSession,
  ManualProposalInput,
  RunConfirmationResponse,
  RunEventEnvelope,
  WorkflowGraph,
  WorkspaceMessageResponse,
  WorkbenchState,
} from './types';
import { applyRunEvent, shouldRefetchWorkspaceState } from './store-events';
import {
  appendManualEditInput,
  manualEditInputFromSession,
} from './workbench-edit-session';

export { applyRunEvent } from './store-events';

type LoadStatus = 'idle' | 'loading' | 'ready' | 'error';
type QueueRunOptions = { forceRerun?: boolean };
type PresenceByActor = Record<string, unknown>;
type WorkspaceSnapshot = { state: WorkbenchState; canvas: CanvasDocument };

type WorkbenchStore = {
  status: LoadStatus;
  error: string | null;
  connection: ConnectionStatus;
  canvasStatus: LoadStatus;
  canvasError: string | null;
  canvasConnection: ConnectionStatus;
  canvas: CanvasDocument | null;
  selectedCanvasNodeIds: string[];
  presenceByActor: PresenceByActor;
  state: WorkbenchState | null;
  editSession: ManualEditSession | null;
  bootstrap: (workspaceId?: string | null) => Promise<void>;
  hydrate: (workspaceId: string) => Promise<void>;
  createWorkspace: () => Promise<void>;
  setInitialState: (state: WorkbenchState) => void;
  setConnection: (status: ConnectionStatus) => void;
  setCanvasSelection: (nodeIds: string[]) => void;
  applyEvent: (event: RunEventEnvelope) => void;
  sendMessage: (text: string, canvasContext?: CanvasMessageContext) => Promise<void>;
  queueRun: (options?: QueueRunOptions) => Promise<void>;
  interruptRun: (runId?: string) => Promise<void>;
  exportWorkflow: () => Promise<WorkflowGraph | null>;
  undoVersion: () => Promise<void>;
  restoreVersion: (versionId: string) => Promise<void>;
  saveLayout: (positions: LayoutPositionUpdate[]) => Promise<void>;
  selectOutput: (outputId: string) => Promise<void>;
  selectProvider: (providerId: string) => Promise<void>;
  appendManualEdit: (input: ManualProposalInput) => Promise<void>;
  commitManualEdits: () => Promise<void>;
  discardManualEdits: () => void;
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
    snapshotRefresh = fetchWorkspaceSnapshot(workspaceId)
      .then(({ state, canvas }) =>
        set((current) =>
          current.state?.workspace.id === workspaceId
            ? {
                status: 'ready',
                canvasStatus: 'ready',
                state,
                canvas,
                error: null,
                canvasError: null,
                editSession: compatibleEditSession(current.editSession, state),
              }
            : {},
        ),
      )
      .catch((error) => {
        const message =
          error instanceof Error ? error.message : 'workspace state request failed';
        set((current) =>
          current.state?.workspace.id === workspaceId
            ? { error: message, canvasError: message, canvasStatus: 'error' }
            : {},
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
  canvasStatus: 'idle',
  canvasError: null,
  canvasConnection: 'offline',
  canvas: null,
  selectedCanvasNodeIds: [],
  presenceByActor: {},
  state: null,
  editSession: null,
  bootstrap: async (workspaceId) => {
    if (workspaceId) {
      await get().hydrate(workspaceId);
      return;
    }

    set({ status: 'loading', error: null });
    try {
      const workspaces = await fetchWorkspaces();
      const workspace = workspaces[0] ?? (await createWorkspace());
      const snapshot = await fetchWorkspaceSnapshot(workspace.id);
      set(snapshotReady(snapshot));
    } catch (error) {
      set(snapshotError(error, 'workspace bootstrap request failed'));
    }
  },
  hydrate: async (workspaceId) => {
    set({ status: 'loading', canvasStatus: 'loading', error: null, canvasError: null });
    try {
      const snapshot = await fetchWorkspaceSnapshot(workspaceId);
      set(snapshotReady(snapshot));
    } catch (error) {
      set(snapshotError(error, 'workspace state request failed'));
    }
  },
  createWorkspace: async () => {
    set({ status: 'loading', canvasStatus: 'loading', error: null, canvasError: null });
    try {
      const workspace = await createWorkspace();
      const snapshot = await fetchWorkspaceSnapshot(workspace.id);
      set(snapshotReady(snapshot));
    } catch (error) {
      set(snapshotError(error, 'workspace create request failed'));
    }
  },
  setInitialState: (state) =>
    set({
      status: 'ready',
      canvasStatus: 'idle',
      state,
      canvas: null,
      error: null,
      canvasError: null,
      editSession: null,
    }),
  setConnection: (connection) => {
    set({ connection, canvasConnection: connection });
    const state = get().state;
    if (connection === 'live' && state) {
      refreshSnapshot(state.workspace.id);
    }
  },
  setCanvasSelection: (selectedCanvasNodeIds) => set({ selectedCanvasNodeIds }),
  sendMessage: async (text, canvasContext) => {
    const trimmed = text.trim();
    if (!trimmed) {
      return;
    }

    const state = get().state;
    if (!state) {
      return;
    }
    if (hasDirtyEdits(get().editSession)) {
      set((current) => ({
        state: current.state
          ? appendSystemError(current.state, '请先提交或放弃手动编辑后再发送 Agent 请求。')
          : current.state,
      }));
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
  queueRun: async (options = {}) => {
    const state = get().state;
    if (!state) {
      return;
    }
    if (hasDirtyEdits(get().editSession)) {
      set((current) => ({
        state: current.state
          ? appendSystemError(current.state, '请先提交或放弃手动编辑后再运行 Queue。')
          : current.state,
      }));
      return;
    }

    try {
      const response = await queueWorkspaceRun(state.workspace.id, options);
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
      set({ state: next, status: 'ready', error: null, editSession: null });
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
      set({ state: next, status: 'ready', error: null, editSession: null });
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
      set({ state: next, status: 'ready', error: null, editSession: null });
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
      set((current) => ({
        state: next,
        status: 'ready',
        error: null,
        editSession: compatibleEditSession(current.editSession, next),
      }));
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
      set((current) => ({
        state: next,
        status: 'ready',
        error: null,
        editSession: compatibleEditSession(current.editSession, next),
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'workspace provider request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  appendManualEdit: async (input) => {
    const state = get().state;
    if (!state) {
      return;
    }
    if (state.pendingProposal) {
      const message = '请先应用或忽略待审核 proposal 后再编辑。';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
      throw new Error(message);
    }

    try {
      const editSession = appendManualEditInput(
        get().editSession,
        state.workspace.versionId,
        input,
      );
      set({ editSession });
    } catch (error) {
      const normalized =
        error instanceof Error ? error : new Error('manual edit request failed');
      set((current) => ({
        state: current.state ? appendSystemError(current.state, normalized.message) : current.state,
      }));
      throw normalized;
    }
  },
  commitManualEdits: async () => {
    const state = get().state;
    const editSession = get().editSession;
    if (!state || !hasDirtyEdits(editSession)) {
      return;
    }
    if (editSession.baseVersionId !== state.workspace.versionId) {
      const message = '手动编辑基于旧版本，请刷新后重试。';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
        editSession: null,
      }));
      throw new Error(message);
    }

    try {
      const next = await createManualWorkspaceProposal(
        state.workspace.id,
        manualEditInputFromSession(editSession),
      );
      set({ state: next, status: 'ready', error: null, editSession: null });
    } catch (error) {
      const normalized =
        error instanceof Error ? error : new Error('manual edit request failed');
      set((current) => ({
        state: current.state ? appendSystemError(current.state, normalized.message) : current.state,
      }));
      throw normalized;
    }
  },
  discardManualEdits: () => set({ editSession: null }),
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
      set({ state: next, status: 'ready', error: null, editSession: null });
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
      set({ state: next, status: 'ready', error: null, editSession: null });
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
      set({ state: next, status: 'ready', error: null, editSession: null });
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

function hasDirtyEdits(session: ManualEditSession | null): session is ManualEditSession {
  return Boolean(session && session.ops.length > 0);
}

function compatibleEditSession(
  session: ManualEditSession | null,
  state: WorkbenchState,
): ManualEditSession | null {
  return session?.baseVersionId === state.workspace.versionId ? session : null;
}

async function fetchWorkspaceSnapshot(workspaceId: string): Promise<WorkspaceSnapshot> {
  const [state, canvas] = await Promise.all([
    fetchWorkspaceState(workspaceId),
    fetchWorkspaceCanvas(workspaceId),
  ]);
  return { state, canvas };
}

function snapshotReady(snapshot: WorkspaceSnapshot) {
  return {
    status: 'ready' as const,
    canvasStatus: 'ready' as const,
    state: snapshot.state,
    canvas: snapshot.canvas,
    error: null,
    canvasError: null,
    editSession: null,
  };
}

function snapshotError(error: unknown, fallback: string) {
  const message = error instanceof Error ? error.message : fallback;
  return {
    status: 'error' as const,
    canvasStatus: 'error' as const,
    error: message,
    canvasError: message,
  };
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
        return step
          ? { ...node, status: step.state, provider: step.provider, cached: step.cached }
          : node;
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
