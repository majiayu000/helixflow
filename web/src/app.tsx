import { useCallback, useEffect, useMemo, useState } from 'react';
import { connectWorkspaceEvents, fetchWorkspaceList } from './api';
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

export function App({ initialState, workspaceId }: AppProps) {
  const initialWorkspaceId = useMemo(
    () => workspaceId ?? workspaceIdFromUrl() ?? 'local-comfyui-agent',
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
  const hydrate = useWorkbenchStore((store) => store.hydrate);
  const setInitialState = useWorkbenchStore((store) => store.setInitialState);
  const setConnection = useWorkbenchStore((store) => store.setConnection);
  const applyEvent = useWorkbenchStore((store) => store.applyEvent);
  const sendMessage = useWorkbenchStore((store) => store.sendMessage);
  const requestRun = useWorkbenchStore((store) => store.requestRun);
  const applyProposal = useWorkbenchStore((store) => store.applyProposal);
  const dismissProposal = useWorkbenchStore((store) => store.dismissProposal);
  const approveConfirmation = useWorkbenchStore((store) => store.approveConfirmation);
  const holdConfirmation = useWorkbenchStore((store) => store.holdConfirmation);
  const activeState = initialState ?? state;

  const refreshWorkspaces = useCallback(async () => {
    try {
      setWorkspaceList(await fetchWorkspaceList());
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
    void hydrate(selectedWorkspaceId).then(refreshWorkspaces);
  }, [hydrate, initialState, refreshWorkspaces, selectedWorkspaceId, setInitialState]);

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

  const running = activeState.run.status === 'running' || activeState.run.status === 'estimating';
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
  const workspace = activeState.workspace.id;

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
    openWorkspace(`chat-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`);
  };

  return (
    <main className="wb">
      <TopBar
        busy={busy}
        connection={connection}
        historyOpen={historyOpen}
        onAgentRun={() => void runAction(() => requestRun(workspace))}
        onHistory={() => setHistoryOpen((open) => !open)}
        onNewWorkspace={createWorkspace}
        onQueue={() => void runAction(() => requestRun(workspace))}
        runDisabled={queueDisabled}
        running={running}
        state={activeState}
      />
      {error && <div className="error-banner">{error}</div>}
      <div className="wb-body">
        <div className="chat-column">
          <ChatPane
            busy={busy}
            messages={activeState.chat.messages}
            onApplyProposal={(id) => runAction(() => applyProposal(workspace, id))}
            onDismissProposal={(id) => runAction(() => dismissProposal(workspace, id))}
            onSend={(text) => runAction(() => sendMessage(workspace, text))}
            pendingProposal={activeState.pendingProposal}
          />
        </div>
        <section className="wb-canvas">
          <GraphCanvas
            graph={activeState.graph}
            pendingProposal={activeState.pendingProposal}
            run={activeState.run}
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
            onApprove={(id) => runAction(() => approveConfirmation(workspace, id))}
            onHold={(id) => runAction(() => holdConfirmation(workspace, id))}
          />
          <RunDock run={activeState.run} />
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
