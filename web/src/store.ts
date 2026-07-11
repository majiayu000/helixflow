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
} from './api';
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
  hasDirtyEdits,
  manualEditInputFromSession,
} from './workbench-edit-session';
import { isAbortError, WorkspaceRequestScope } from './workspace-request-scope';
import {
  fetchWorkspaceSnapshot,
  snapshotError,
  snapshotReady,
} from './workspace-snapshot';
import type { WorkbenchStore } from './store-types';
import { WorkspaceActionGuard, WorkspaceChangedError } from './workspace-action-guard';

export { applyRunEvent } from './store-events';

export const useWorkbenchStore = create<WorkbenchStore>((set, get) => {
  const requestScope = new WorkspaceRequestScope();
  let snapshotRefresh: { generation: number; promise: Promise<void> } | null = null;
  let trailingSnapshotGeneration: number | null = null;
  const actionGuard = new WorkspaceActionGuard(requestScope);
  const activateWorkspace = (workspaceId: string | null) => {
    const activation = requestScope.begin(workspaceId);
    snapshotRefresh = null;
    trailingSnapshotGeneration = null;
    set({
      workspaceGeneration: activation.generation,
      activeWorkspaceId: workspaceId,
      selectedCanvasNodeIds: [],
      presenceByActor: {},
    });
    return activation;
  };
  const adoptWorkspace = (generation: number, workspaceId: string) => {
    if (!requestScope.adopt(generation, workspaceId)) return false;
    set({ activeWorkspaceId: workspaceId });
    return true;
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
  activeWorkspaceId: null,
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
      if (!adoptWorkspace(activation.generation, workspace.id)) return;
      const snapshot = await fetchWorkspaceSnapshot(workspace.id, activation.signal);
      if (!requestScope.isActive(activation.generation, workspace.id)) return;
      set(snapshotReady(snapshot));
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(activation.generation)) return;
      set(snapshotError(error, 'workspace bootstrap request failed'));
    }
  },
  hydrate: async (workspaceId) => {
    const previousWorkspaceId = get().state?.workspace.id ?? null;
    const activation = activateWorkspace(workspaceId);
    set({ status: 'loading', canvasStatus: 'loading', error: null, canvasError: null });
    try {
      const snapshot = await fetchWorkspaceSnapshot(workspaceId, activation.signal);
      if (!requestScope.isActive(activation.generation, workspaceId)) return;
      set(snapshotReady(snapshot));
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(activation.generation, workspaceId)) return;
      if (previousWorkspaceId) activateWorkspace(previousWorkspaceId);
      set(snapshotError(error, 'workspace state request failed'));
    }
  },
  createWorkspace: async () => {
    const previousWorkspaceId = get().state?.workspace.id ?? null;
    const activation = activateWorkspace(null);
    set({ status: 'loading', canvasStatus: 'loading', error: null, canvasError: null });
    try {
      const workspace = await createWorkspace();
      if (!adoptWorkspace(activation.generation, workspace.id)) return;
      const snapshot = await fetchWorkspaceSnapshot(workspace.id, activation.signal);
      if (!requestScope.isActive(activation.generation, workspace.id)) return;
      set(snapshotReady(snapshot));
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(activation.generation)) return;
      if (previousWorkspaceId) activateWorkspace(previousWorkspaceId);
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
    if (!requestScope.isActive(generation, state.workspace.id)) return;
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
    if (!requestScope.isActive(generation, state.workspace.id)) {
      actionGuard.fail('workspace changed before the canvas comment could be sent');
    }
    try {
      const canvas = await submitCanvasCommentOpRequest(
        state.workspace.id,
        input,
        requestScope.signal(),
      );
      if (!requestScope.isActive(generation, state.workspace.id)) {
        actionGuard.fail('workspace changed while the canvas comment was sending');
      }
      set({ canvas, canvasStatus: 'ready', canvasError: null });
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(generation, state.workspace.id)) {
        actionGuard.fail('workspace changed while the canvas comment was sending');
      }
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
    const request = {
      workspaceId: state.workspace.id,
      baseVersionId: state.workspace.versionId,
      graph: workflowGraphFromState(state),
    };
    const generation = requestScope.currentGeneration();
    if (!requestScope.isActive(generation, request.workspaceId)) {
      throw new WorkspaceChangedError('workspace changed before the message could be sent');
    }
    if (hasDirtyEdits(get().editSession)) {
      const message = '请先提交或放弃手动编辑后再发送 Agent 请求。';
      set((current) => ({
        state: current.state
          ? appendSystemError(current.state, message)
          : current.state,
      }));
      throw new Error(message);
    }

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
      if (!requestScope.isActive(generation, request.workspaceId)) {
        actionGuard.fail('workspace changed while the message was sending');
      }
      if (response.messages.some((message) => message.kind === 'proposal_applied')) {
        const next = await fetchWorkspaceState(request.workspaceId, requestScope.signal());
        if (!requestScope.isActive(generation, request.workspaceId)) {
          actionGuard.fail('workspace changed while the message was sending');
        }
        set({ state: next, status: 'ready', error: null, editSession: null });
        return;
      }
      set((current) => ({
        state: current.state ? applyMessageResponse(current.state, response) : current.state,
      }));
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(generation, request.workspaceId)) {
        actionGuard.fail('workspace changed while the message was sending');
      }
      const normalized = error instanceof Error ? error : new Error('workspace message request failed');
      set((current) => ({
        state: current.state
          ? appendChatMessages(current.state, [
              {
                id: `msg_system_${Date.now()}`,
                role: 'system',
                kind: 'run_failed',
                text: normalized.message,
                time: messageTime(),
              },
            ])
          : current.state,
      }));
      throw normalized;
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
    actionGuard.assertActive(state.workspace.id, 'queueing the run');
    if (hasDirtyEdits(get().editSession)) {
      set((current) => ({
        state: current.state
          ? appendSystemError(current.state, '请先提交或放弃手动编辑后再运行 Queue。')
          : current.state,
      }));
      return;
    }

    try {
      const response = await actionGuard.run(state.workspace.id, 'queueing the run', () =>
        queueWorkspaceRun(state.workspace.id, options));
      set((current) => ({
        state: current.state ? applyRunConfirmation(current.state, response) : current.state,
      }));
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
      const response = await actionGuard.run(state.workspace.id, 'interrupting the run', () =>
        interruptRunRequest(targetRunId));
      set((current) => ({
        state: current.state ? applyRunConfirmation(current.state, response) : current.state,
      }));
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
      return await actionGuard.run(state.workspace.id, 'exporting the workflow', () =>
        exportWorkflowVersion(versionId));
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
      const next = await actionGuard.run(state.workspace.id, 'undoing the version', () =>
        undoWorkspaceVersion(state.workspace.id));
      set({ state: next, status: 'ready', error: null, editSession: null });
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
      const next = await actionGuard.run(state.workspace.id, 'restoring the version', () =>
        restoreWorkspaceVersion(state.workspace.id, versionId));
      set({ state: next, status: 'ready', error: null, editSession: null });
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
      const next = await actionGuard.run(state.workspace.id, 'saving the layout', () =>
        saveWorkspaceLayout(state.workspace.id, {
          baseVersionId: state.workspace.versionId,
          positions,
        }));
      set({ state: next, status: 'ready', error: null, editSession: null });
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
      const next = await actionGuard.run(state.workspace.id, 'selecting the output', () =>
        selectOutputRequest(outputId));
      set((current) => ({
        state: next,
        status: 'ready',
        error: null,
        editSession: compatibleEditSession(current.editSession, next),
      }));
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
      const message = error instanceof Error ? error.message : 'output select request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  acceptOutput: async (outputId) => {
    const state = get().state;
    if (!state) {
      return;
    }
    try {
      const next = await actionGuard.run(state.workspace.id, 'accepting the output', () =>
        acceptOutputRequest(outputId));
      set((current) => ({
        state: next,
        status: 'ready',
        error: null,
        editSession: compatibleEditSession(current.editSession, next),
      }));
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
      const message = error instanceof Error ? error.message : 'output accept request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
    }
  },
  rejectOutput: async (outputId, rerun = false) => {
    const state = get().state;
    if (!state) {
      return;
    }
    try {
      const next = await actionGuard.run(state.workspace.id, 'rejecting the output', () =>
        rejectOutputRequest(outputId, rerun));
      set((current) => ({
        state: next,
        status: 'ready',
        error: null,
        editSession: compatibleEditSession(current.editSession, next),
      }));
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
    if (!requestScope.isActive(generation, state.workspace.id)) {
      actionGuard.fail('workspace changed before the provider could be selected');
    }
    try {
      const next = await selectWorkspaceProvider(
        state.workspace.id,
        providerId,
        requestScope.signal(),
      );
      if (!requestScope.isActive(generation, state.workspace.id)) {
        actionGuard.fail('workspace changed while the provider was being selected');
      }
      set((current) => ({
        state: next,
        status: 'ready',
        error: null,
        editSession: compatibleEditSession(current.editSession, next),
      }));
    } catch (error) {
      if (isAbortError(error) || !requestScope.isActive(generation, state.workspace.id)) {
        actionGuard.fail('workspace changed while the provider was being selected');
      }
      const message = error instanceof Error ? error.message : 'workspace provider request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
      throw error instanceof Error ? error : new Error(message);
    }
  },
  appendManualEdit: async (input) => {
    const state = get().state;
    if (!state) {
      return;
    }
    actionGuard.assertActive(state.workspace.id, 'editing the workspace');
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
    actionGuard.assertActive(state.workspace.id, 'committing manual edits');
    if (editSession.baseVersionId !== state.workspace.versionId) {
      const message = '手动编辑基于旧版本，请刷新后重试。';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
      throw new Error(message);
    }

    try {
      const next = await actionGuard.run(state.workspace.id, 'committing manual edits', () =>
        createManualWorkspaceProposal(
          state.workspace.id,
          manualEditInputFromSession(editSession),
        ));
      set({ state: next, status: 'ready', error: null, editSession: null });
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
    actionGuard.assertActive(state.workspace.id, 'confirming the run');
    const previousRun = state.run;
    const previousPendingConfirmation = state.pendingConfirmation;

    set((current) => ({
      state: current.state ? markRunConfirming(current.state, runId) : current.state,
    }));
    try {
      const response = await actionGuard.run(state.workspace.id, 'confirming the run', () =>
        confirmWorkspaceRun(state.workspace.id, runId));
      set((current) => ({
        state: current.state ? applyRunConfirmation(current.state, response) : current.state,
      }));
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
      const response = await actionGuard.run(state.workspace.id, 'holding the run', () =>
        holdWorkspaceRun(state.workspace.id, runId));
      set((current) => ({
        state: current.state ? applyRunConfirmation(current.state, response) : current.state,
      }));
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
      const next = await actionGuard.run(state.workspace.id, 'creating the proposal', () =>
        createManualWorkspaceProposal(state.workspace.id, input));
      set({ state: next, status: 'ready', error: null, editSession: null });
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
      const next = await actionGuard.run(state.workspace.id, 'applying the proposal', () =>
        applyWorkspaceProposal(state.workspace.id, proposalId));
      set({ state: next, status: 'ready', error: null, editSession: null });
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
      const next = await actionGuard.run(state.workspace.id, 'dismissing the proposal', () =>
        dismissWorkspaceProposal(state.workspace.id, proposalId));
      set({ state: next, status: 'ready', error: null, editSession: null });
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
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
