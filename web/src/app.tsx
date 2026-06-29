import { useCallback, useEffect, useMemo, useState } from 'react';
import { connectWorkspaceEvents, fetchWorkspaces } from './api';
import { ChatPane } from './components/chat-pane';
import { GraphCanvas } from './components/graph-canvas';
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
  const running = uiState.run.status === 'running' || uiState.run.status === 'estimating';
  const hasProviderNodes = activeState.graph.nodes.some((node) => Boolean(node.provider));
  const providerReady = activeState.providers.providers.some(
    (provider) => provider.id === activeState.providers.defaultProvider && provider.enabled,
  );
  const queueDisabled =
    busy ||
    running ||
    activeState.graph.nodes.length === 0 ||
    (hasProviderNodes && !providerReady) ||
    Boolean(activeState.pendingConfirmation) ||
    Boolean(activeState.pendingProposal);

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

  return (
    <main className="wb">
      <TopBar
        busy={busy}
        connection={connection}
        historyOpen={historyOpen}
        onAgentRun={() => void runAction(() => sendMessage('运行当前 workflow'))}
        onHistory={() => setHistoryOpen((open) => !open)}
        onNewWorkspace={createWorkspace}
        onQueue={() => void runAction(() => sendMessage('运行当前 workflow'))}
        runDisabled={queueDisabled}
        running={running}
        state={uiState}
      />
      {error && <div className="error-banner">{error}</div>}
      <div className="wb-body">
        <div className="chat-column">
          <ChatPane
            busy={busy}
            messages={activeState.chat.messages}
            onApplyProposal={(id) => runAction(() => applyProposal(id))}
            onDismissProposal={(id) => runAction(() => dismissProposal(id))}
            onSend={(text) => runAction(() => sendMessage(text))}
            pendingProposal={activeState.pendingProposal}
          />
        </div>
        <section className="wb-canvas">
          <GraphCanvas
            graph={activeState.graph}
            pendingProposal={activeState.pendingProposal}
            run={uiState.run}
          />
          <HistoryPanel
            history={activeState.history}
            currentWorkspaceId={activeState.workspace.id}
            onClose={() => setHistoryOpen(false)}
            onOpenWorkspace={openWorkspace}
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
          <OutputsStrip outputs={activeState.outputs} />
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
