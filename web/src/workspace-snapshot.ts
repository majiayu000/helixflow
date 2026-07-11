import { fetchWorkspaceCanvas, fetchWorkspaceState } from './api';
import type { CanvasDocument, WorkbenchState } from './types';

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
