import { useCallback, useEffect, useState } from 'react';
import { fetchWorkspaces } from './api';
import {
  planNavigationRequest,
  resolveDirtyNavigation,
  type DirtyNavigationDecision,
  type PendingNavigation,
} from './dirty-navigation';
import { useWorkbenchStore } from './store';
import type { WorkbenchState, WorkspaceSummary } from './types';

type NavigationAction = () => Promise<unknown>;

type WorkbenchNavigationInput = {
  activeState: WorkbenchState | null;
  commitManualEdits: NavigationAction;
  createWorkspace: NavigationAction;
  dirtyEditCount: number;
  discardManualEdits: () => void;
  exportWorkflow: () => Promise<WorkbenchState['workflowGraph'] | null>;
  initialWorkspaceId: string | null;
  restoreVersion: (versionId: string) => Promise<unknown>;
  undoVersion: NavigationAction;
};

export function useWorkbenchNavigation({
  activeState,
  commitManualEdits,
  createWorkspace,
  dirtyEditCount,
  discardManualEdits,
  exportWorkflow,
  initialWorkspaceId,
  restoreVersion,
  undoVersion,
}: WorkbenchNavigationInput) {
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState(initialWorkspaceId);
  const [busy, setBusy] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [pendingNavigation, setPendingNavigation] = useState<PendingNavigation | null>(null);
  const [navigationBusy, setNavigationBusy] = useState(false);
  const [workspaceList, setWorkspaceList] = useState<WorkspaceSummary[]>([]);
  const [workspaceListError, setWorkspaceListError] = useState<string | null>(null);

  useEffect(() => {
    setSelectedWorkspaceId(initialWorkspaceId);
  }, [initialWorkspaceId]);

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

  const runAction = useCallback(
    async (action: () => Promise<void>, propagateError = false) => {
      setBusy(true);
      try {
        await action();
        await refreshWorkspaces();
      } catch (error) {
        if (propagateError) throw error;
      } finally {
        setBusy(false);
      }
    },
    [refreshWorkspaces],
  );

  const performNavigation = useCallback(
    async (target: PendingNavigation) => {
      if (target.kind === 'workspace') {
        setHistoryOpen(false);
        setSelectedWorkspaceId(target.workspaceId);
        updateWorkspaceUrl(target.workspaceId);
        return;
      }
      if (target.kind === 'undo') {
        await runAction(async () => void (await undoVersion()), true);
        return;
      }
      if (target.kind === 'restore') {
        await runAction(async () => void (await restoreVersion(target.versionId)), true);
        return;
      }
      await runAction(
        async () => {
          await createWorkspace();
          const workspaceId = useWorkbenchStore.getState().state?.workspace.id;
          if (!workspaceId) throw new Error('workspace create did not return an active workspace');
          setHistoryOpen(false);
          setSelectedWorkspaceId(workspaceId);
          updateWorkspaceUrl(workspaceId);
        },
        true,
      );
    },
    [createWorkspace, restoreVersion, runAction, undoVersion],
  );

  const navigationLocked = busy || navigationBusy || Boolean(pendingNavigation);

  const requestNavigation = useCallback(
    (target: PendingNavigation) => {
      if (!activeState) return;
      const plan = planNavigationRequest({
        hasDirtyEdits: dirtyEditCount > 0,
        isCurrentWorkspace:
          target.kind === 'workspace' && target.workspaceId === activeState.workspace.id,
        locked: navigationLocked,
      });
      if (plan === 'ignore') return;
      if (plan === 'prompt') {
        setPendingNavigation(target);
        return;
      }
      void performNavigation(target).catch(() => undefined);
    },
    [activeState, dirtyEditCount, navigationLocked, performNavigation],
  );

  const decideDirtyNavigation = useCallback(
    (decision: DirtyNavigationDecision) => {
      const target = pendingNavigation;
      if (!target || navigationBusy) return;
      if (decision === 'cancel') {
        setPendingNavigation(null);
        return;
      }
      setNavigationBusy(true);
      void resolveDirtyNavigation(decision, {
        commit: () => runAction(async () => void (await commitManualEdits()), true),
        discard: discardManualEdits,
        isDirty: () => Boolean(useWorkbenchStore.getState().editSession?.ops.length),
        navigate: () => performNavigation(target),
      })
        .then(() => setPendingNavigation(null))
        .catch(() => undefined)
        .finally(() => setNavigationBusy(false));
    },
    [commitManualEdits, discardManualEdits, navigationBusy, pendingNavigation, performNavigation, runAction],
  );

  const exportCurrentWorkflow = useCallback(() => {
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
      .catch(() => undefined)
      .finally(() => setBusy(false));
  }, [activeState, exportWorkflow]);

  return {
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
  };
}

export function workspaceIdFromUrl(): string | null {
  if (typeof window === 'undefined') return null;
  return new URLSearchParams(window.location.search).get('workspace_id');
}

function updateWorkspaceUrl(workspaceId: string) {
  if (typeof window === 'undefined') return;
  const url = new URL(window.location.href);
  url.searchParams.set('workspace_id', workspaceId);
  window.history.pushState(null, '', url);
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
