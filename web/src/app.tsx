import { useCallback, useEffect, useMemo, useState } from 'react';
import { connectWorkspaceEvents, fetchWorkspaces } from './api';
import { ArtifactStage, hasPreviewArtifact } from './components/artifact-stage';
import { ChatPane } from './components/chat-pane';
import { GraphCanvas } from './components/graph-canvas';
import { ManualProposalPanel } from './components/manual-proposal-panel';
import { ConfirmModal, HistoryPanel, OutputsStrip, RunDock } from './components/run-panels';
import { TopBar } from './components/top-bar';
import { useWorkbenchStore } from './store';
import type { WorkbenchState, WorkspaceSummary } from './types';

type AppProps = {
  initialState?: WorkbenchState;
  workspaceId?: string;
};

type RunSnapshot = NonNullable<WorkbenchState['run']>;
type UiWorkbenchState = WorkbenchState & { run: RunSnapshot };

export function App({ initialState, workspaceId }: AppProps) {
  const initialWorkspaceId = useMemo(
    () => workspaceId ?? workspaceIdFromUrl(),
    [workspaceId],
  );
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState(initialWorkspaceId);
  const [busy, setBusy] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [selectedCanvasNodeIds, setSelectedCanvasNodeIds] = useState<string[]>([]);
  const [workspaceList, setWorkspaceList] = useState<WorkspaceSummary[]>([]);
  const [workspaceListError, setWorkspaceListError] = useState<string | null>(null);
  const status = useWorkbenchStore((store) => store.status);
  const error = useWorkbenchStore((store) => store.error);
  const connection = useWorkbenchStore((store) => store.connection);
  const state = useWorkbenchStore((store) => store.state);
  const bootstrap = useWorkbenchStore((store) => store.bootstrap);
  const createWorkspaceAction = useWorkbenchStore((store) => store.createWorkspace);
  const setInitialState = useWorkbenchStore((store) => store.setInitialState);
  const setConnection = useWorkbenchStore((store) => store.setConnection);
  const applyEvent = useWorkbenchStore((store) => store.applyEvent);
  const sendMessage = useWorkbenchStore((store) => store.sendMessage);
  const applyProposal = useWorkbenchStore((store) => store.applyProposal);
  const dismissProposal = useWorkbenchStore((store) => store.dismissProposal);
  const confirmRun = useWorkbenchStore((store) => store.confirmRun);
  const holdRun = useWorkbenchStore((store) => store.holdRun);
  const createManualProposal = useWorkbenchStore((store) => store.createManualProposal);
  const queueRun = useWorkbenchStore((store) => store.queueRun);
  const interruptRun = useWorkbenchStore((store) => store.interruptRun);
  const exportWorkflow = useWorkbenchStore((store) => store.exportWorkflow);
  const undoVersion = useWorkbenchStore((store) => store.undoVersion);
  const restoreVersion = useWorkbenchStore((store) => store.restoreVersion);
  const saveLayout = useWorkbenchStore((store) => store.saveLayout);
  const selectOutput = useWorkbenchStore((store) => store.selectOutput);
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
    if (!activeState?.workspace.id) return;
    return connectWorkspaceEvents(activeState.workspace.id, {
      onEvent: applyEvent,
      onStatus: setConnection,
    });
  }, [activeState?.workspace.id, applyEvent, setConnection]);

  if (!activeState) {
    return <LoadingShell status={status} error={error} />;
  }

  const uiState = stateForUi(activeState);
  const activeRun = Boolean(
    activeState.run &&
      (activeState.run.status === 'queued' ||
        activeState.run.status === 'running' ||
        activeState.run.status === 'estimating'),
  );
  const hasProviderNodes = activeState.graph.nodes.some((node) => Boolean(node.provider));
  const providerReady = activeState.providers.providers.some(
    (provider) => provider.id === activeState.providers.defaultProvider && provider.enabled,
  );
  const queueDisabled =
    busy ||
    activeRun ||
    activeState.graph.nodes.length === 0 ||
    (hasProviderNodes && !providerReady) ||
    Boolean(activeState.pendingConfirmation) ||
    Boolean(activeState.pendingProposal);
  const versionHistoryCount = activeState.history.filter((item) => item.kind === 'version').length;
  const undoDisabled = busy || versionHistoryCount < 2;

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
        onQueue={() => {
          if (activeRun) {
            void interruptRun();
          } else {
            void runAction(() => queueRun());
          }
        }}
        onUndo={() => void runAction(() => undoVersion())}
        runDisabled={activeRun ? false : queueDisabled}
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
        <section className="wb-canvas">
          {hasPreviewArtifact(activeState.outputs) ? (
            <ArtifactStage outputs={activeState.outputs} />
          ) : (
            <>
              <GraphCanvas
                graph={activeState.graph}
                onSelectionChange={setSelectedCanvasNodeIds}
                onSaveLayout={(positions) => runAction(() => saveLayout(positions))}
                pendingProposal={activeState.pendingProposal}
                run={uiState.run}
                versionId={activeState.workspace.versionId}
                workspaceId={activeState.workspace.id}
              />
              <ManualProposalPanel
                busy={busy}
                state={activeState}
                onCreateProposal={(input) => runAction(() => createManualProposal(input))}
              />
            </>
          )}
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
        <p>COMFYUI AGENT</p>
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
