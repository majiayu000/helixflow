import type { StoreApi } from 'zustand';
import { fetchWorkspaceCanvas, fetchWorkspaceState } from './api';
import type { ConnectionStatus } from './api';
import { preserveRetryNotices } from './store-events';
import { compatibleEditSession } from './store-model';
import type { WorkbenchStore } from './store-types';
import type { CanvasDocument, WorkbenchState } from './types';
import { isAbortError, type WorkspaceRequestScope } from './workspace-request-scope';

export type WorkspaceSnapshot = {
  state: WorkbenchState;
  canvas: CanvasDocument;
};

export async function fetchWorkspaceSnapshot(
  workspaceId: string,
  signal?: AbortSignal,
): Promise<WorkspaceSnapshot> {
  const [state, canvas] = await Promise.all([
    fetchWorkspaceState(workspaceId, signal),
    fetchWorkspaceCanvas(workspaceId, signal),
  ]);
  return { state, canvas };
}

export function snapshotReady(snapshot: WorkspaceSnapshot) {
  return {
    status: 'ready' as const,
    canvasStatus: 'ready' as const,
    state: snapshot.state,
    canvas: snapshot.canvas,
    error: null,
    canvasError: null,
    editSession: null,
    selectedCanvasNodeIds: [],
    presenceByActor: {},
  };
}

export function snapshotError(error: unknown, fallback: string) {
  const message = error instanceof Error ? error.message : fallback;
  return {
    status: 'error' as const,
    canvasStatus: 'error' as const,
    error: message,
    canvasError: message,
  };
}

type StoreGet = StoreApi<WorkbenchStore>['getState'];
type StoreSet = StoreApi<WorkbenchStore>['setState'];

export class WorkspaceSnapshotCoordinator {
  private liveGeneration: number | null = null;
  private refreshState: { generation: number; promise: Promise<void> } | null = null;
  private trailingGeneration: number | null = null;

  constructor(
    private readonly requestScope: WorkspaceRequestScope,
    private readonly get: StoreGet,
    private readonly set: StoreSet,
  ) {}

  reset(): void {
    this.liveGeneration = null;
    this.refreshState = null;
    this.trailingGeneration = null;
  }

  setConnection(connection: ConnectionStatus): void {
    this.set({ connection, canvasConnection: connection });
    const state = this.get().state;
    if (connection !== 'live' || !state) return;
    const generation = this.requestScope.currentGeneration();
    if (this.liveGeneration !== generation) {
      this.liveGeneration = generation;
      return;
    }
    this.refresh(state.workspace.id);
  }

  refresh(workspaceId: string): void {
    const generation = this.requestScope.currentGeneration();
    if (
      this.requestScope.currentWorkspaceId() !== workspaceId ||
      this.requestScope.signal()?.aborted
    ) return;
    if (this.refreshState?.generation === generation) {
      this.trailingGeneration = generation;
      return;
    }
    const promise = fetchWorkspaceSnapshot(workspaceId, this.requestScope.signal())
      .then(({ state, canvas }) => this.applySnapshot(generation, workspaceId, state, canvas))
      .catch((error) => this.applyError(generation, workspaceId, error))
      .finally(() => this.finishRefresh(generation, workspaceId));
    this.refreshState = { generation, promise };
  }

  private applySnapshot(
    generation: number,
    workspaceId: string,
    state: WorkbenchState,
    canvas: CanvasDocument,
  ): void {
    this.set((current) =>
      this.requestScope.isActive(generation, workspaceId) &&
      this.trailingGeneration !== generation &&
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
    );
  }

  private applyError(generation: number, workspaceId: string, error: unknown): void {
    if (isAbortError(error) || !this.requestScope.isActive(generation, workspaceId)) return;
    const message = error instanceof Error ? error.message : 'workspace state request failed';
    this.set((current) =>
      current.state?.workspace.id === workspaceId
        ? { error: message, canvasError: message, canvasStatus: 'error' }
        : {},
    );
  }

  private finishRefresh(generation: number, workspaceId: string): void {
    if (this.refreshState?.generation !== generation) return;
    this.refreshState = null;
    if (this.trailingGeneration === generation) {
      this.trailingGeneration = null;
      this.refresh(workspaceId);
    }
  }
}
