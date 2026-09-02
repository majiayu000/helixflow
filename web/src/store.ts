import { create } from 'zustand';
import {
  CANVAS_PRESENCE_TTL_MS,
  isLocalCanvasActor,
} from './canvas-presence';
import {
  applyWorkspaceProposal,
  confirmWorkspaceRun,
  createManualWorkspaceProposal,
  createWorkspaceConversation,
  createWorkspace,
  dismissWorkspaceProposal,
  exportWorkflowVersion,
  fetchWorkspaceState,
  fetchWorkspaces,
  holdWorkspaceRun,
  interruptRun as interruptRunRequest,
  interruptWorkspaceAgent,
  queueWorkspaceRun,
  restoreWorkspaceVersion,
  sendCanvasPresence as sendCanvasPresenceRequest,
  selectOutput as selectOutputRequest,
  acceptOutput as acceptOutputRequest,
  rejectOutput as rejectOutputRequest,
  selectWorkspaceProvider,
  sendWorkspaceMessage,
  submitCanvasCommentOp as submitCanvasCommentOpRequest,
  undoWorkspaceVersion,
  uploadWorkspaceImage,
} from './api';
import {
  applyRunEvent,
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
import { appendManualEditInput, hasDirtyEdits } from './workbench-edit-session';
import { isAbortError, WorkspaceRequestScope } from './workspace-request-scope';
import {
  fetchWorkspaceSnapshot,
  snapshotError,
  snapshotReady,
  WorkspaceSnapshotCoordinator,
} from './workspace-snapshot';
import type { WorkbenchStore } from './store-types';
import { createUploadImageAction } from './store-upload';
import { StorePersistence } from './store-persistence';
import { WorkspaceActionGuard, WorkspaceChangedError } from './workspace-action-guard';

export { applyRunEvent } from './store-events';

export const useWorkbenchStore = create<WorkbenchStore>((set, get) => {
  const requestScope = new WorkspaceRequestScope();
  const presenceExpiryTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const actionGuard = new WorkspaceActionGuard(requestScope);
  const persistence = new StorePersistence(get, set, actionGuard);
  const snapshots = new WorkspaceSnapshotCoordinator(requestScope, get, set);
  const clearPresenceExpiryTimers = () => {
    for (const timer of presenceExpiryTimers.values()) clearTimeout(timer);
    presenceExpiryTimers.clear();
  };
  const activateWorkspace = (workspaceId: string | null) => {
    persistence.reset();
    clearPresenceExpiryTimers();
    const activation = requestScope.begin(workspaceId);
    snapshots.reset();
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
  streamSeqs: {},
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
  setConnection: (connection) => snapshots.setConnection(connection),
  setCanvasSelection: (selectedCanvasNodeIds) => set({ selectedCanvasNodeIds }),
  applyCanvasPresence: (presence) => {
    const actorId = presence.actor.actorId;
    if (isLocalCanvasActor(actorId)) return;
    const generation = requestScope.currentGeneration();
    const workspaceId = requestScope.currentWorkspaceId();
    const previousTimer = presenceExpiryTimers.get(actorId);
    if (previousTimer) clearTimeout(previousTimer);
    presenceExpiryTimers.set(actorId, setTimeout(() => {
      presenceExpiryTimers.delete(actorId);
      if (!requestScope.isActive(generation, workspaceId ?? undefined)) return;
      set((current) => {
        if (!(actorId in current.presenceByActor)) return {};
        const presenceByActor = { ...current.presenceByActor };
        delete presenceByActor[actorId];
        return { presenceByActor };
      });
    }, CANVAS_PRESENCE_TTL_MS));
    set((current) => ({
      presenceByActor: {
        ...current.presenceByActor,
        [actorId]: presence,
      },
    }));
  },
  sendCanvasPresence: async (presence) => {
    const state = get().state;
    if (!state) return;
    const generation = requestScope.currentGeneration();
    if (!requestScope.isActive(generation, state.workspace.id)) return;
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
  uploadImage: createUploadImageAction(set, get, requestScope),
  createConversation: async () => {
    const state = get().state;
    if (!state) throw new Error('workspace is not loaded');
    const conversation = await createWorkspaceConversation(
      state.workspace.id,
      '新对话',
      requestScope.signal(),
    );
    set((current) => ({
      state: current.state
        ? {
            ...current.state,
            chat: {
              ...current.state.chat,
              activeConversationId: conversation.id,
              conversations: [conversation, ...(current.state.chat.conversations ?? [])],
            },
          }
        : current.state,
    }));
    return conversation.id;
  },
  sendMessage: async (text, canvasContext, conversationId, turnMode) => {
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
      await persistence.flushManual();
    }
    const flushed = get().state;
    if (!flushed) {
      return;
    }
    if (!requestScope.isActive(generation, request.workspaceId)) {
      throw new WorkspaceChangedError('workspace changed before the message could be sent');
    }
    request.baseVersionId = flushed.workspace.versionId;
    request.graph = workflowGraphFromState(flushed);

    set({
      state: appendChatMessages(flushed, [
        {
          id: `msg_user_${Date.now()}`,
          role: 'user',
          kind: 'text',
          text: trimmed,
          time: messageTime(),
          conversationId: conversationId ?? flushed.chat.activeConversationId ?? undefined,
        },
      ]),
    });

    try {
      const response = await sendWorkspaceMessage(request.workspaceId, {
        baseVersionId: request.baseVersionId,
        canvasContext,
        userMessage: trimmed,
        graph: request.graph,
        conversationId: conversationId ?? flushed.chat.activeConversationId ?? undefined,
        turnMode,
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
    set((current) => ({
      streamSeqs: {
        ...current.streamSeqs,
        [event.run_id]: Math.max(current.streamSeqs[event.run_id] ?? 0, event.seq),
      },
    }));
    const needsSnapshot = shouldRefetchWorkspaceState(state, event);
    set({ state: applyRunEvent(state, event) });
    if (needsSnapshot) {
      snapshots.refresh(state.workspace.id);
    }
  },
  queueRun: async (options = {}) => {
    const state = get().state;
    if (!state) {
      return;
    }
    actionGuard.assertActive(state.workspace.id, 'queueing the run');
    if (hasDirtyEdits(get().editSession)) {
      try {
        await persistence.flushManual();
      } catch (error) {
        if (error instanceof WorkspaceChangedError) throw error;
        return;
      }
    }
    const flushed = get().state;
    if (!flushed) {
      return;
    }
    actionGuard.assertActive(flushed.workspace.id, 'queueing the run');

    try {
      const response = await actionGuard.run(flushed.workspace.id, 'queueing the run', () =>
        queueWorkspaceRun(flushed.workspace.id, options));
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
  interruptAgent: async () => {
    const state = get().state;
    if (!state) throw new Error('workspace is not loaded');
    try {
      await interruptWorkspaceAgent(state.workspace.id);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'agent interrupt request failed';
      set((current) => ({
        state: current.state ? appendSystemError(current.state, message) : current.state,
      }));
      throw error;
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
  saveCanvasSnapshot: (update) => persistence.saveCanvas(update),
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

    await persistence.enqueueManual(async () => {
      const current = get().state;
      if (!current) {
        return;
      }
      if (current.pendingProposal) {
        const message = '请先应用或忽略待审核 proposal 后再编辑。';
        set((latest) => ({
          state: latest.state ? appendSystemError(latest.state, message) : latest.state,
        }));
        throw new Error(message);
      }
      try {
        const editSession = appendManualEditInput(
          get().editSession,
          current.workspace.versionId,
          input,
        );
        set({ editSession });
      } catch (error) {
        const normalized =
          error instanceof Error ? error : new Error('canvas edit request failed');
        set((latest) => ({
          editSession: null,
          state: latest.state ? appendSystemError(latest.state, normalized.message) : latest.state,
        }));
        throw normalized;
      }
      await persistence.persistManual();
    });
  },
  commitManualEdits: async () => {
    await persistence.flushManual();
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
