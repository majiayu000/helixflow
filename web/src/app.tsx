import { useCallback, useEffect, useMemo, useState } from 'react';
import { connectWorkspaceEvents, fetchWorkspaces } from './api';
import { ArtifactStage, hasPreviewArtifact } from './components/artifact-stage';
import { ChatPane } from './components/chat-pane';
import { GraphCanvas } from './components/graph-canvas';
import { ManualProposalPanel } from './components/manual-proposal-panel';
import { ConfirmModal, HistoryPanel, OutputsStrip, RunDock } from './components/run-panels';
import { TopBar } from './components/top-bar';
import { useWorkbenchStore } from './store';
import {
  graphStateFromCanvasDocument,
  type ManualEditSession,
  type WorkbenchState,
  type WorkspaceSummary,
} from './types';
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
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState(initialWorkspaceId);
  const [busy, setBusy] = useState(false);
  const [forceRerun, setForceRerun] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [selectedCanvasNodeIds, setSelectedCanvasNodeIds] = useState<string[]>([]);
  const [workspaceList, setWorkspaceList] = useState<WorkspaceSummary[]>([]);
  const [workspaceListError, setWorkspaceListError] = useState<string | null>(null);
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
  const exportWorkflow = useWorkbenchStore((store) => store.exportWorkflow);
  const undoVersion = useWorkbenchStore((store) => store.undoVersion);
  const restoreVersion = useWorkbenchStore((store) => store.restoreVersion);
  const selectOutput = useWorkbenchStore((store) => store.selectOutput);
  const acceptOutput = useWorkbenchStore((store) => store.acceptOutput);
  const rejectOutput = useWorkbenchStore((store) => store.rejectOutput);
  const selectProvider = useWorkbenchStore((store) => store.selectProvider);
  const activeState = initialState ?? state;

  const refreshWorkspaces = useCallback(async () => {
    try {
      setWorkspaceList(await fetchWorkspaces());
      setWorkspaceListError(null);
    } catch (error) {
      setWorkspaceListError(
        error instanceof Error ? error.message : 'workspace list request failed',
      );
    }
  }, []);

  useEffect(() => {
    if (initialState) {
      setInitialState(initialState);
      return;
    }
    void bootstrap(selectedWorkspaceId).then(refreshWorkspaces);
  }, [bootstrap, initialState, refreshWorkspaces, selectedWorkspaceId, setInitialState]);

  useEffect(() => {
    setSelectedWorkspaceId(initialWorkspaceId);
  }, [initialWorkspaceId]);

  useEffect(() => {
    setSelectedCanvasNodeIds([]);
  }, [activeWorkspaceId, workspaceGeneration]);

  useEffect(() => {
    if (!activeState?.workspace.id) return;
    if (activeWorkspaceId !== activeState.workspace.id) return;
    const generation = workspaceGeneration;
    const isCurrentSubscription = () =>
      useWorkbenchStore.getState().workspaceGeneration === generation &&
      useWorkbenchStore.getState().state?.workspace.id === activeState.workspace.id;
    return connectWorkspaceEvents(activeState.workspace.id, {
      getLastSeq: () => useWorkbenchStore.getState().state?.eventSeq ?? 0,
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
  const dirtyEditCount = editSession?.ops.length ?? 0;
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
  const undoDisabled = busy || versionHistoryCount < 2;
  const showArtifactPreview = hasPreviewArtifact(activeState.outputs) && dirtyEditCount === 0;

  const runAction = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
      await refreshWorkspaces();
    } finally {
      setBusy(false);
    }
  };

  const openWorkspace = (id: string) => {
    setHistoryOpen(false);
    setSelectedWorkspaceId(id);
    updateWorkspaceUrl(id);
  };

  const createWorkspace = () => {
    void runAction(async () => {
      await createWorkspaceAction();
      const workspaceId = useWorkbenchStore.getState().state?.workspace.id;
      if (workspaceId) {
        setHistoryOpen(false);
        setSelectedWorkspaceId(workspaceId);
        updateWorkspaceUrl(workspaceId);
      }
    });
  };

  const exportCurrentWorkflow = () => {
    if (!activeState?.workspace.versionId) return;
    setBusy(true);
    void exportWorkflow()
      .then((graph) => {
        if (graph) {
          downloadWorkflowJson(
            graph,
            `${activeState.workspace.name}-${activeState.workspace.versionId}.json`,
          );
        }
      })
      .finally(() => setBusy(false));
  };

  return (
    <main className="wb">
      <TopBar
        agentRunDisabled={queueDisabled}
        busy={busy}
        connection={connection}
        exportDisabled={busy || !activeState.workspace.versionId}
        historyOpen={historyOpen}
        onAgentRun={() => void runAction(() => sendMessage('运行当前 workflow'))}
        onExport={exportCurrentWorkflow}
        onHistory={() => setHistoryOpen((open) => !open)}
        onNewWorkspace={createWorkspace}
        onProviderSelect={(providerId) => void runAction(() => selectProvider(providerId))}
        onQueue={() => {
          if (activeRun) {
            void interruptRun();
          } else {
            void runAction(() => queueRun({ forceRerun }));
          }
        }}
        onUndo={() => void runAction(() => undoVersion())}
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
            busy={busy}
            messages={activeState.chat.messages}
            onApplyProposal={(id) => runAction(() => applyProposal(id))}
            onDismissProposal={(id) => runAction(() => dismissProposal(id))}
            editSessionSummary={
              dirtyEditCount > 0
                ? {
                    baseVersionId: editSession?.baseVersionId ?? activeState.workspace.versionId,
                    count: dirtyEditCount,
                    items: editSummary,
                  }
                : null
            }
            onCommitEdits={() => runAction(() => commitManualEdits())}
            onDiscardEdits={discardManualEdits}
            onSend={(text) =>
              runAction(() =>
                sendMessage(text, {
                  selection: { nodeIds: selectedCanvasNodeIds },
                }),
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
            onCreateProposal={(input) => runAction(() => appendManualEdit(input))}
            onCommentOp={(input) => runAction(() => submitCanvasCommentOp(input))}
            onPresenceChange={(presence) => void sendCanvasPresence(presence)}
            onQueueRun={() => {
              if (!queueDisabled) void runAction(() => queueRun({ forceRerun }));
            }}
            onRequestNodeProposal={(nodeId) =>
              runAction(() =>
                sendMessage(`围绕选中节点 ${nodeId} 生成最小修改 proposal。`, {
                  selection: { nodeIds: [nodeId] },
                }),
              )
            }
            onSelectionChange={(nodeIds) => {
              setSelectedCanvasNodeIds(nodeIds);
              setCanvasSelection(nodeIds);
            }}
            onSetParam={(nodeId, key, value) => {
              return runAction(() =>
                appendManualEdit(
                  buildSetParamEditInput(
                    activeState.workspace.versionId,
                    previewState.workflowGraph,
                    nodeId,
                    key,
                    value,
                  ),
                ),
              );
            }}
            onSelectOutput={(id) => void runAction(() => selectOutput(id))}
            outputs={activeState.outputs}
            pendingProposal={activeState.pendingProposal}
            presenceByActor={presenceByActor}
            queueRunDisabled={queueDisabled}
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
          <ManualProposalPanel
            busy={busy}
            state={previewState}
            onCreateProposal={(input) => runAction(() => appendManualEdit(input))}
          />
          <HistoryPanel
            busy={busy}
            history={activeState.history}
            currentWorkspaceId={activeState.workspace.id}
            currentVersionId={activeState.workspace.versionId}
            onClose={() => setHistoryOpen(false)}
            onOpenWorkspace={openWorkspace}
            onRestoreVersion={(id) => void runAction(() => restoreVersion(id))}
            open={historyOpen}
            workspaceListError={workspaceListError}
            workspaces={workspaceList}
          />
          <ConfirmModal
            busy={busy}
            confirmation={activeState.pendingConfirmation}
            onApprove={(id) => runAction(() => confirmRun(id))}
            onHold={(id) => runAction(() => holdRun(id))}
          />
          <RunDock run={uiState.run} />
          <OutputsStrip
            busy={busy}
            outputs={activeState.outputs}
            onSelect={(id) => void runAction(() => selectOutput(id))}
            onAccept={(id) => void runAction(() => acceptOutput(id))}
            onReject={(id, rerun) => void runAction(() => rejectOutput(id, rerun))}
          />
        </section>
      </div>
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

function downloadWorkflowJson(graph: WorkbenchState['workflowGraph'], filename: string) {
  if (!graph || typeof document === 'undefined') return;
  const blob = new Blob([`${JSON.stringify(graph, null, 2)}\n`], {
    type: 'application/json',
  });
  const link = document.createElement('a');
  link.href = URL.createObjectURL(blob);
  link.download = safeDownloadName(filename);
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(link.href);
}

function safeDownloadName(filename: string): string {
  const sanitized = filename
    .trim()
    .replace(/\.json$/i, '')
    .replace(/[^a-z0-9._-]+/gi, '-')
    .replace(/^-+|-+$/g, '');
  return `${sanitized || 'helixflow-workflow'}.json`;
}

function workspaceIdFromUrl(): string | null {
  if (typeof window === 'undefined') return null;
  return new URLSearchParams(window.location.search).get('workspace_id');
}

function updateWorkspaceUrl(workspaceId: string) {
  if (typeof window === 'undefined') return;
  const url = new URL(window.location.href);
  url.searchParams.set('workspace_id', workspaceId);
  window.history.pushState(null, '', url);
}
