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
  sendCanvasPresence as sendCanvasPresenceRequest,
  selectOutput as selectOutputRequest,
  acceptOutput as acceptOutputRequest,
  rejectOutput as rejectOutputRequest,
  selectWorkspaceProvider,
  sendWorkspaceMessage,
  submitCanvasCommentOp as submitCanvasCommentOpRequest,
  undoWorkspaceVersion,
  type ConnectionStatus,
} from './api';
import type {
  CanvasDocument,
  CanvasCommentOpInput,
  CanvasMessageContext,
  CanvasPresence,
  LayoutPositionUpdate,
  ManualEditSession,
  ManualProposalInput,
  RunEventEnvelope,
  WorkflowGraph,
  WorkbenchState,
} from './types';
import {
  applyRunEvent,
  preserveRetryNotices,
  shouldRefetchWorkspaceState,
} from './store-events';
import {
  appendChatMessages,
  appendSystemError,
  applyMessageResponse,
  applyRunConfirmation,
  compatibleEditSession,
  markRunConfirming,
  messageTime,
  workflowGraphFromState,
} from './store-model';
import {
  appendManualEditInput,
  manualEditInputFromSession,
} from './workbench-edit-session';
import { isAbortError, WorkspaceRequestScope } from './workspace-request-scope';
import {
  fetchWorkspaceSnapshot,
  snapshotError,
  snapshotReady,
} from './workspace-snapshot';

export { applyRunEvent } from './store-events';

type LoadStatus = 'idle' | 'loading' | 'ready' | 'error';
type QueueRunOptions = { forceRerun?: boolean };
type PresenceByActor = Record<string, CanvasPresence>;

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
  workspaceGeneration: number;
  bootstrap: (workspaceId?: string | null) => Promise<void>;
  hydrate: (workspaceId: string) => Promise<void>;
  createWorkspace: () => Promise<void>;
  setInitialState: (state: WorkbenchState) => void;
  setConnection: (status: ConnectionStatus) => void;
  setCanvasSelection: (nodeIds: string[]) => void;
  applyCanvasPresence: (presence: CanvasPresence) => void;
  sendCanvasPresence: (presence: CanvasPresence) => Promise<void>;
  submitCanvasCommentOp: (input: CanvasCommentOpInput) => Promise<void>;
  applyEvent: (event: RunEventEnvelope, generation?: number) => void;
  sendMessage: (text: string, canvasContext?: CanvasMessageContext) => Promise<void>;
  queueRun: (options?: QueueRunOptions) => Promise<void>;
  interruptRun: (runId?: string) => Promise<void>;
  exportWorkflow: () => Promise<WorkflowGraph | null>;
  undoVersion: () => Promise<void>;
  restoreVersion: (versionId: string) => Promise<void>;
  saveLayout: (positions: LayoutPositionUpdate[]) => Promise<void>;
  selectOutput: (outputId: string) => Promise<void>;
  acceptOutput: (outputId: string) => Promise<void>;
  rejectOutput: (outputId: string, rerun?: boolean) => Promise<void>;
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
  const requestScope = new WorkspaceRequestScope();
  let snapshotRefresh: { generation: number; promise: Promise<void> } | null = null;
  let trailingSnapshotGeneration: number | null = null;
  const activateWorkspace = (workspaceId: string | null) => {
    const activation = requestScope.begin(workspaceId);
    snapshotRefresh = null;
    trailingSnapshotGeneration = null;
    set({
      workspaceGeneration: activation.generation,
      selectedCanvasNodeIds: [],
      presenceByActor: {},
    });
    return activation;
  };
  const refreshSnapshot = (workspaceId: string) => {
    const generation = requestScope.currentGeneration();
    if (requestScope.currentWorkspaceId() !== workspaceId || requestScope.signal()?.aborted) return;
    if (snapshotRefresh?.generation === generation) {
      trailingSnapshotGeneration = generation;
      return;
    }
    const promise = fetchWorkspaceSnapshot(workspaceId, requestScope.signal())
      .then(({ state, canvas }) =>
        set((current) =>
          requestScope.isActive(generation, workspaceId) &&
            trailingSnapshotGeneration !== generation &&
            current.state?.workspace.id === workspaceId
            ? {
                status: 'ready',
                canvasStatus: 'ready',
                state: preserveRetryNotices(current.state, state),
                canvas,
                error: null,
                canvasError: null,
                editSession: compatibleEditSession(current.editSession, state),
              }
            : {},
        ),
      )
      .catch((error) => {
        if (isAbortError(error) || !requestScope.isActive(generation, workspaceId)) return;
        const message =
          error instanceof Error ? error.message : 'workspace state request failed';
        set((current) =>
          current.state?.workspace.id === workspaceId
            ? { error: message, canvasError: message, canvasStatus: 'error' }
            : {},
        );
      })
      .finally(() => {
        if (snapshotRefresh?.generation !== generation) return;
        snapshotRefresh = null;
        if (trailingSnapshotGeneration === generation) {
          trailingSnapshotGeneration = null;
          refreshSnapshot(workspaceId);
        }
      });
    snapshotRefresh = { generation, promise };
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
  workspaceGeneration: 0,
  bootstrap: async (workspaceId) => {
    if (workspaceId) {
      await get().hydrate(workspaceId);
      return;
    }

    const activation = activateWorkspace(null);
    set({ status: 'loading', error: null });
    try {
      const workspaces = await fetchWorkspaces(activation.signal);
      const workspace = workspaces[0] ?? (await createWorkspace());
      if (!requestScope.adopt(activation.generation, workspace.id)) return;
      const snapshot = await fetchWorkspaceSnapshot(workspace.id, activation.signal);
      if (!requestScope.isActive(activation.generation, workspace.id)) return;
      set(snapshotReady(snapshot));
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(activation.generation)) return;
      set(snapshotError(error, 'workspace bootstrap request failed'));
    }
  },
  hydrate: async (workspaceId) => {
    const activation = activateWorkspace(workspaceId);
    set({ status: 'loading', canvasStatus: 'loading', error: null, canvasError: null });
    try {
      const snapshot = await fetchWorkspaceSnapshot(workspaceId, activation.signal);
      if (!requestScope.isActive(activation.generation, workspaceId)) return;
      set(snapshotReady(snapshot));
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(activation.generation, workspaceId)) return;
      set(snapshotError(error, 'workspace state request failed'));
    }
  },
  createWorkspace: async () => {
    const activation = activateWorkspace(null);
    set({ status: 'loading', canvasStatus: 'loading', error: null, canvasError: null });
    try {
      const workspace = await createWorkspace();
      if (!requestScope.adopt(activation.generation, workspace.id)) return;
      const snapshot = await fetchWorkspaceSnapshot(workspace.id, activation.signal);
      if (!requestScope.isActive(activation.generation, workspace.id)) return;
      set(snapshotReady(snapshot));
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(activation.generation)) return;
      set(snapshotError(error, 'workspace create request failed'));
    }
  },
  setInitialState: (state) => {
    activateWorkspace(state.workspace.id);
    set({
      status: 'ready',
      canvasStatus: 'idle',
      state,
      canvas: null,
      error: null,
      canvasError: null,
      editSession: null,
      selectedCanvasNodeIds: [],
      presenceByActor: {},
    });
  },
  setConnection: (connection) => {
    set({ connection, canvasConnection: connection });
    const state = get().state;
    if (connection === 'live' && state) {
      refreshSnapshot(state.workspace.id);
    }
  },
  setCanvasSelection: (selectedCanvasNodeIds) => set({ selectedCanvasNodeIds }),
  applyCanvasPresence: (presence) =>
    set((current) => ({
      presenceByActor: {
        ...current.presenceByActor,
        [presence.actor.actorId]: presence,
      },
    })),
  sendCanvasPresence: async (presence) => {
    const state = get().state;
    if (!state) return;
    const generation = requestScope.currentGeneration();
    set((current) => ({
      presenceByActor: {
        ...current.presenceByActor,
        [presence.actor.actorId]: presence,
      },
    }));
    try {
      await sendCanvasPresenceRequest(state.workspace.id, presence, requestScope.signal());
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(generation, state.workspace.id)) return;
      set({ canvasConnection: 'offline' });
    }
  },
  submitCanvasCommentOp: async (input) => {
    const state = get().state;
    if (!state) return;
    const generation = requestScope.currentGeneration();
    try {
      const canvas = await submitCanvasCommentOpRequest(
        state.workspace.id,
        input,
        requestScope.signal(),
      );
      if (!requestScope.isActive(generation, state.workspace.id)) return;
      set({ canvas, canvasStatus: 'ready', canvasError: null });
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(generation, state.workspace.id)) return;
      const normalized =
        error instanceof Error ? error : new Error('canvas comment request failed');
      set((current) => ({
        canvasStatus: 'error',
        canvasError: normalized.message,
        state: current.state ? appendSystemError(current.state, normalized.message) : current.state,
      }));
      throw normalized;
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
    const generation = requestScope.currentGeneration();
    if (!requestScope.isActive(generation, request.workspaceId)) return;

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
      }, requestScope.signal());
      if (!requestScope.isActive(generation, request.workspaceId)) return;
      if (response.messages.some((message) => message.kind === 'proposal_applied')) {
        const next = await fetchWorkspaceState(request.workspaceId, requestScope.signal());
        if (!requestScope.isActive(generation, request.workspaceId)) return;
        set({ state: next, status: 'ready', error: null, editSession: null });
        return;
      }
      set((current) => ({
        state: current.state ? applyMessageResponse(current.state, response) : current.state,
      }));
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(generation, request.workspaceId)) return;
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
  applyEvent: (event, generation = requestScope.currentGeneration()) => {
    if (!requestScope.isActive(generation, event.workspace_id)) return;
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
  acceptOutput: async (outputId) => {
    if (!get().state) {
      return;
    }
    try {
      const next = await acceptOutputRequest(outputId);
      set((current) => ({
        state: next,
        status: 'ready',
        error: null,
        editSession: compatibleEditSession(current.editSession, next),
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'output accept request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  rejectOutput: async (outputId, rerun = false) => {
    if (!get().state) {
      return;
    }
    try {
      const next = await rejectOutputRequest(outputId, rerun);
      set((current) => ({
        state: next,
        status: 'ready',
        error: null,
        editSession: compatibleEditSession(current.editSession, next),
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : 'output reject request failed';
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

    const generation = requestScope.currentGeneration();
    try {
      const next = await selectWorkspaceProvider(
        state.workspace.id,
        providerId,
        requestScope.signal(),
      );
      if (!requestScope.isActive(generation, state.workspace.id)) return;
      set((current) => ({
        state: next,
        status: 'ready',
        error: null,
        editSession: compatibleEditSession(current.editSession, next),
      }));
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(generation, state.workspace.id)) return;
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

function hasDirtyEdits(session: ManualEditSession | null): session is ManualEditSession {
  return Boolean(session && session.ops.length > 0);
}
