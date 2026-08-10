import { useEffect, useMemo, useState } from 'react';
import { connectWorkspaceEvents } from './api';
import { ArtifactStage, hasPreviewArtifact } from './components/artifact-stage';
import { ChatPane } from './components/chat-pane';
import { DirtyNavigationDialog } from './components/dirty-navigation-dialog';
import { GraphCanvas } from './components/graph-canvas';
import { ManualProposalPanel } from './components/manual-proposal-panel';
import { ConfirmModal, HistoryPanel, OutputsStrip, RunDock } from './components/run-panels';
import { TopBar } from './components/top-bar';
import { useWorkbenchStore } from './store';
import {
  graphStateFromCanvasDocument,
  type ManualEditSession,
  type WorkbenchState,
} from './types';
import { useWorkbenchNavigation, workspaceIdFromUrl } from './use-workbench-navigation';
import {
  buildSetParamEditInput,
  deriveQueueLockReason,
  manualEditSummary,
  previewWorkbenchStateWithManualEdits,
} from './workbench-edit-session';

type AppProps = {
  initialState?: WorkbenchState;
  initialEditSession?: ManualEditSession | null;
  workspaceId?: string;
};

type RunSnapshot = NonNullable<WorkbenchState['run']>;
type UiWorkbenchState = WorkbenchState & { run: RunSnapshot };

export function App({ initialState, initialEditSession, workspaceId }: AppProps) {
  const initialWorkspaceId = useMemo(
    () => workspaceId ?? workspaceIdFromUrl(),
    [workspaceId],
  );
  const showAdvancedEditPanel = useMemo(() => advancedEditPanelEnabledFromUrl(), []);
  const [forceRerun, setForceRerun] = useState(false);
  const [selectedCanvasNodeIds, setSelectedCanvasNodeIds] = useState<string[]>([]);
  const [selectedConversationId, setSelectedConversationId] = useState<string | null>(null);
  const status = useWorkbenchStore((store) => store.status);
  const error = useWorkbenchStore((store) => store.error);
  const connection = useWorkbenchStore((store) => store.connection);
  const canvas = useWorkbenchStore((store) => store.canvas);
  const presenceByActor = useWorkbenchStore((store) => store.presenceByActor);
  const state = useWorkbenchStore((store) => store.state);
  const storeEditSession = useWorkbenchStore((store) => store.editSession);
  const editSession = initialEditSession ?? storeEditSession;
  const bootstrap = useWorkbenchStore((store) => store.bootstrap);
  const createWorkspaceAction = useWorkbenchStore((store) => store.createWorkspace);
  const setInitialState = useWorkbenchStore((store) => store.setInitialState);
  const setConnection = useWorkbenchStore((store) => store.setConnection);
  const setCanvasSelection = useWorkbenchStore((store) => store.setCanvasSelection);
  const applyCanvasPresence = useWorkbenchStore((store) => store.applyCanvasPresence);
  const applyEvent = useWorkbenchStore((store) => store.applyEvent);
  const workspaceGeneration = useWorkbenchStore((store) => store.workspaceGeneration);
  const activeWorkspaceId = useWorkbenchStore((store) => store.activeWorkspaceId);
  const sendCanvasPresence = useWorkbenchStore((store) => store.sendCanvasPresence);
  const sendMessage = useWorkbenchStore((store) => store.sendMessage);
  const createConversation = useWorkbenchStore((store) => store.createConversation);
  const uploadImage = useWorkbenchStore((store) => store.uploadImage);
  const submitCanvasCommentOp = useWorkbenchStore((store) => store.submitCanvasCommentOp);
  const applyProposal = useWorkbenchStore((store) => store.applyProposal);
  const dismissProposal = useWorkbenchStore((store) => store.dismissProposal);
  const confirmRun = useWorkbenchStore((store) => store.confirmRun);
  const holdRun = useWorkbenchStore((store) => store.holdRun);
  const appendManualEdit = useWorkbenchStore((store) => store.appendManualEdit);
  const commitManualEdits = useWorkbenchStore((store) => store.commitManualEdits);
  const discardManualEdits = useWorkbenchStore((store) => store.discardManualEdits);
  const queueRun = useWorkbenchStore((store) => store.queueRun);
  const interruptRun = useWorkbenchStore((store) => store.interruptRun);
  const interruptAgent = useWorkbenchStore((store) => store.interruptAgent);
  const exportWorkflow = useWorkbenchStore((store) => store.exportWorkflow);
  const undoVersion = useWorkbenchStore((store) => store.undoVersion);
  const restoreVersion = useWorkbenchStore((store) => store.restoreVersion);
  const selectOutput = useWorkbenchStore((store) => store.selectOutput);
  const acceptOutput = useWorkbenchStore((store) => store.acceptOutput);
  const rejectOutput = useWorkbenchStore((store) => store.rejectOutput);
  const selectProvider = useWorkbenchStore((store) => store.selectProvider);
  const activeState = initialState ?? state;
  const dirtyEditCount = editSession?.ops.length ?? 0;
  const {
    busy,
    decideDirtyNavigation,
    exportCurrentWorkflow,
    historyOpen,
    navigationBusy,
    navigationLocked,
    pendingNavigation,
    refreshWorkspaces,
    requestNavigation,
    runAction,
    selectedWorkspaceId,
    setHistoryOpen,
    workspaceList,
    workspaceListError,
  } = useWorkbenchNavigation({
    activeState,
    commitManualEdits,
    createWorkspace: createWorkspaceAction,
    dirtyEditCount,
    discardManualEdits,
    exportWorkflow,
    initialWorkspaceId,
    restoreVersion,
    undoVersion,
  });

  useEffect(() => {
    if (initialState) {
      setInitialState(initialState);
      return;
    }
    void bootstrap(selectedWorkspaceId).then(refreshWorkspaces);
  }, [bootstrap, initialState, refreshWorkspaces, selectedWorkspaceId, setInitialState]);

  useEffect(() => {
    setSelectedCanvasNodeIds([]);
    setSelectedConversationId(null);
  }, [activeWorkspaceId, workspaceGeneration]);

  useEffect(() => {
    if (!activeState) return;
    const conversations = activeState.chat.conversations ?? [];
    if (selectedConversationId && conversations.some((item) => item.id === selectedConversationId)) {
      return;
    }
    setSelectedConversationId(activeState.chat.activeConversationId ?? conversations[0]?.id ?? null);
  }, [activeState, selectedConversationId]);

  useEffect(() => {
    if (!activeState?.workspace.id) return;
    if (activeWorkspaceId !== activeState.workspace.id) return;
    const generation = workspaceGeneration;
    const isCurrentSubscription = () =>
      useWorkbenchStore.getState().workspaceGeneration === generation &&
      useWorkbenchStore.getState().state?.workspace.id === activeState.workspace.id;
    return connectWorkspaceEvents(activeState.workspace.id, {
      getLastSeq: () => useWorkbenchStore.getState().state?.eventSeq ?? 0,
      getStreamSeq: (runId) => {
        const store = useWorkbenchStore.getState();
        if (store.state?.run?.id === runId) {
          return Math.max(store.state.eventSeq, store.streamSeqs[runId] ?? 0);
        }
        return store.streamSeqs[runId] ?? 0;
      },
      isPrimaryStream: (runId) => useWorkbenchStore.getState().state?.run?.id === runId,
      onEvent: (event) => applyEvent(event, generation),
      onPresence: (presence) => {
        if (isCurrentSubscription()) applyCanvasPresence(presence);
      },
      onStatus: (status) => {
        if (isCurrentSubscription()) setConnection(status);
      },
    });
  }, [
    activeState?.workspace.id,
    activeWorkspaceId,
    applyCanvasPresence,
    applyEvent,
    setConnection,
    workspaceGeneration,
  ]);

  if (!activeState) {
    return <LoadingShell status={status} error={error} />;
  }

  const previewState = previewWorkbenchStateWithManualEdits(activeState, editSession);
  const canvasGraph =
    dirtyEditCount === 0 && canvas && canvas.versionId === activeState.workspace.versionId
      ? graphStateFromCanvasDocument(canvas, activeState.graph)
      : undefined;
  const uiState = stateForUi(previewState);
  const editSummary =
    editSession && editSession.baseVersionId === activeState.workspace.versionId
      ? manualEditSummary(editSession)
      : [];
  const activeRun = Boolean(
    activeState.run &&
      (activeState.run.status === 'queued' ||
        activeState.run.status === 'running' ||
        activeState.run.status === 'estimating'),
  );
  const hasProviderNodes = previewState.graph.nodes.some((node) => Boolean(node.provider));
  const providerReady = activeState.providers.runtimeProviders.some(
    (provider) =>
      provider.id === activeState.providers.selectedProvider &&
      provider.enabled &&
      provider.status === 'healthy',
  );
  const selectedProvider = activeState.providers.runtimeProviders.find(
    (provider) =>
      provider.id ===
      (activeState.providers.selectedProvider ?? activeState.providers.defaultProvider),
  );
  const providerMessage =
    selectedProvider?.message ?? (providerReady ? 'Provider ready' : 'Provider unavailable');
  const queueLockReason = deriveQueueLockReason({
    activeRun,
    busy,
    dirtyEditCount,
    graphNodeCount: previewState.graph.nodes.length,
    hasProviderNodes,
    pendingConfirmation: Boolean(activeState.pendingConfirmation),
    pendingProposal: Boolean(activeState.pendingProposal),
    providerMessage,
    providerReady,
  });
  const queueDisabled = queueLockReason.kind !== 'none';
  const versionHistoryCount = activeState.history.filter((item) => item.kind === 'version').length;
  const undoDisabled = navigationLocked || versionHistoryCount < 2;
  const showArtifactPreview = hasPreviewArtifact(activeState.outputs) && dirtyEditCount === 0;
  const conversations = activeState.chat.conversations ?? [];
  const activeConversationId =
    selectedConversationId ?? activeState.chat.activeConversationId ?? conversations[0]?.id ?? null;
  const conversationMessages = activeConversationId
    ? activeState.chat.messages.filter(
        (message) => !message.conversationId || message.conversationId === activeConversationId,
      )
    : activeState.chat.messages;
  const conversationTurns = (activeState.chat.turns ?? []).filter(
    (turn) => turn.conversationId === activeConversationId,
  );

  return (
    <main className="wb">
      <TopBar
        agentRunDisabled={queueDisabled}
        busy={navigationLocked}
        connection={connection}
        exportDisabled={busy || !activeState.workspace.versionId}
        historyOpen={historyOpen}
        onAgentRun={() =>
          void runAction(() => sendMessage('运行当前 workflow', undefined, activeConversationId ?? undefined))
        }
        onCommitEdits={() => void runAction(() => commitManualEdits())}
        onExport={exportCurrentWorkflow}
        onHistory={() => setHistoryOpen((open) => !open)}
        onNewWorkspace={() => requestNavigation({ kind: 'create_workspace' })}
        onProviderSelect={(providerId) => void runAction(() => selectProvider(providerId))}
        onQueue={() => {
          if (activeRun) {
            void runAction(() => interruptRun());
          } else {
            void runAction(() => queueRun({ forceRerun }));
          }
        }}
        onUndo={() => requestNavigation({ kind: 'undo' })}
        runDisabled={activeRun ? false : queueDisabled}
        forceRerun={forceRerun}
        onForceRerunChange={setForceRerun}
        queueLockReason={queueLockReason}
        running={activeRun}
        state={uiState}
        undoDisabled={undoDisabled}
      />
      {error && <div className="error-banner">{error}</div>}
      <div className="wb-body">
        <div className="chat-column">
          <ChatPane
            busy={navigationLocked}
            messages={conversationMessages}
            conversations={conversations}
            turns={conversationTurns}
            activeConversationId={activeConversationId}
            onConversationChange={setSelectedConversationId}
            onNewConversation={() =>
              runAction(async () => {
                const id = await createConversation();
                setSelectedConversationId(id);
              }, true)
            }
            onInterrupt={interruptAgent}
            onApplyProposal={(id) => runAction(() => applyProposal(id))}
            onDismissProposal={(id) => runAction(() => dismissProposal(id))}
            onUploadImage={(file) => runAction(() => uploadImage(file))}
            editSessionSummary={
              dirtyEditCount > 0
                ? {
                    baseVersionId: editSession?.baseVersionId ?? activeState.workspace.versionId,
                    count: dirtyEditCount,
                    items: editSummary,
                  }
                : null
            }
            selectedNodeIds={selectedCanvasNodeIds}
            onCommitEdits={() => runAction(() => commitManualEdits())}
            onDiscardEdits={discardManualEdits}
            onSend={(text) =>
              runAction(
                () =>
                  sendMessage(
                    text,
                    { selection: { nodeIds: selectedCanvasNodeIds } },
                    activeConversationId ?? undefined,
                  ),
                true,
              )
            }
            pendingProposal={activeState.pendingProposal}
            run={activeState.run}
          />
        </div>
        <section className={showArtifactPreview ? 'wb-canvas wb-canvas--with-artifact' : 'wb-canvas'}>
          <GraphCanvas
            graph={previewState.graph}
            canvasGraph={canvasGraph}
            comments={canvas?.comments ?? []}
            onCreateProposal={(input) => runAction(() => appendManualEdit(input), true)}
            onCommentOp={(input) => runAction(() => submitCanvasCommentOp(input), true)}
            onPresenceChange={(presence) => void sendCanvasPresence(presence)}
            onRequestNodeProposal={(nodeId) =>
              runAction(
                () => sendMessage(`围绕选中节点 ${nodeId} 生成最小修改 proposal。`, {
                  selection: { nodeIds: [nodeId] },
                }),
                true,
              )
            }
            onSelectionChange={(nodeIds) => {
              setSelectedCanvasNodeIds(nodeIds);
              setCanvasSelection(nodeIds);
            }}
            onSetParam={(nodeId, key, value) => {
              return runAction(
                () => appendManualEdit(
                    buildSetParamEditInput(
                      activeState.workspace.versionId,
                      previewState.workflowGraph,
                      nodeId,
                      key,
                      value,
                    ),
                  ),
                true,
              );
            }}
            onSelectOutput={(id) => void runAction(() => selectOutput(id))}
            outputs={activeState.outputs}
            pendingProposal={activeState.pendingProposal}
            presenceByActor={presenceByActor}
            run={uiState.run}
            versionId={activeState.workspace.versionId}
            workflowGraph={previewState.workflowGraph}
            workspaceId={activeState.workspace.id}
          />
          {showArtifactPreview && (
            <div className="canvas-artifact-preview">
              <ArtifactStage outputs={activeState.outputs} />
            </div>
          )}
          {showAdvancedEditPanel && (
            <ManualProposalPanel
              busy={navigationLocked}
              state={previewState}
              onCreateProposal={(input) => runAction(() => appendManualEdit(input), true)}
            />
          )}
          <HistoryPanel
            busy={navigationLocked}
            history={activeState.history}
            currentWorkspaceId={activeState.workspace.id}
            currentVersionId={activeState.workspace.versionId}
            currentConnectorId={selectedProvider?.id ?? activeState.providers.defaultProvider}
            migrationBlocked={dirtyEditCount > 0}
            onClose={() => setHistoryOpen(false)}
            onOpenWorkspace={(workspaceId) => requestNavigation({ kind: 'workspace', workspaceId })}
            onRestoreVersion={(versionId) => requestNavigation({ kind: 'restore', versionId })}
            onMigrationApplied={(nextState) => setInitialState(nextState)}
            open={historyOpen}
            workspaceListError={workspaceListError}
            workspaces={workspaceList}
          />
          <ConfirmModal
            busy={navigationLocked}
            confirmation={activeState.pendingConfirmation}
            onApprove={(id) => runAction(() => confirmRun(id))}
            onHold={(id) => runAction(() => holdRun(id))}
          />
          <RunDock run={uiState.run} />
          <OutputsStrip
            busy={navigationLocked}
            outputs={activeState.outputs}
            onSelect={(id) => void runAction(() => selectOutput(id))}
            onAccept={(id) => void runAction(() => acceptOutput(id))}
            onReject={(id, rerun) => void runAction(() => rejectOutput(id, rerun))}
          />
        </section>
      </div>
      <DirtyNavigationDialog
        busy={busy || navigationBusy}
        target={pendingNavigation}
        onDecision={decideDirtyNavigation}
      />
    </main>
  );
}

function LoadingShell({ status, error }: { status: string; error: string | null }) {
  return (
    <main className="wb wb-loading">
      <div className="loading-panel">
        <p>HELIXFLOW</p>
        <h1>{status === 'error' ? '工作区加载失败' : '正在连接真实工作区'}</h1>
        <span>{error ?? '读取本地 workspace state...'}</span>
      </div>
    </main>
  );
}

function stateForUi(state: WorkbenchState): UiWorkbenchState {
  return {
    ...state,
    run: state.run ?? emptyRunSnapshot(),
  };
}

function emptyRunSnapshot(): RunSnapshot {
  return {
    id: '',
    label: 'No run',
    status: 'queued',
    steps: [],
    cost: {
      estimate: 0,
      actual: 0,
      currency: 'USD',
    },
  };
}

function advancedEditPanelEnabledFromUrl(): boolean {
  if (typeof window === 'undefined') return false;
  return new URLSearchParams(window.location.search).get('advanced_edit') === '1';
}
