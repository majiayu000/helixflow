import type { StoreApi } from 'zustand';
import { createManualWorkspaceProposal, saveWorkspaceCanvasSnapshot } from './api';
import { appendSystemError } from './store-model';
import type { WorkbenchStore } from './store-types';
import type { CanvasSnapshotUpdate, ManualEditSession } from './types';
import { WorkspaceActionGuard, WorkspaceChangedError } from './workspace-action-guard';
import {
  hasDirtyEdits,
  layoutOpsFromSnapshot,
  manualEditInputFromSession,
  partitionManualEditOps,
} from './workbench-edit-session';

type StoreGet = () => WorkbenchStore;
type StoreSet = StoreApi<WorkbenchStore>['setState'];

export class StorePersistence {
  private manualChain = Promise.resolve();
  private canvasChain = Promise.resolve();

  constructor(
    private readonly get: StoreGet,
    private readonly set: StoreSet,
    private readonly guard: WorkspaceActionGuard,
  ) {}

  reset(): void {
    this.manualChain = Promise.resolve();
    this.canvasChain = Promise.resolve();
  }

  enqueueManual(work: () => Promise<void>): Promise<void> {
    const run = this.manualChain.then(work, work);
    this.manualChain = run.then(() => undefined, () => undefined);
    return run;
  }

  async persistManual(): Promise<void> {
    const state = this.get().state;
    const editSession = this.get().editSession;
    if (!state || !hasDirtyEdits(editSession)) return;
    await this.persistCapturedManual(state.workspace.id, editSession);
  }

  flushManual(): Promise<void> {
    const state = this.get().state;
    const editSession = this.get().editSession;
    if (!state || !hasDirtyEdits(editSession)) return Promise.resolve();
    this.guard.assertActive(state.workspace.id, 'saving canvas edits');
    return this.persistCapturedManual(state.workspace.id, editSession);
  }

  saveCanvas(update: CanvasSnapshotUpdate): Promise<void> {
    const workspaceId = this.get().state?.workspace.id;
    if (!workspaceId) return Promise.resolve();
    const run = this.canvasChain.then(async () => {
      const current = this.get();
      if (current.state?.workspace.id !== workspaceId || !current.canvas) {
        this.guard.fail('workspace changed while saving the canvas snapshot');
      }
      try {
        const canvas = await this.guard.run(workspaceId, 'saving the canvas snapshot', () =>
          saveWorkspaceCanvasSnapshot(workspaceId, {
            versionId: current.state!.workspace.versionId,
            baseRevision: current.canvas!.revision,
            ...update,
          }));
        this.set((latest) => latest.state?.workspace.id === workspaceId
          ? { canvas, canvasStatus: 'ready', canvasError: null }
          : {});
      } catch (error) {
        if (error instanceof WorkspaceChangedError) throw error;
        const message = error instanceof Error ? error.message : 'canvas snapshot request failed';
        this.set((latest) => latest.state?.workspace.id === workspaceId
          ? { canvasStatus: 'error', canvasError: message }
          : {});
        throw error instanceof Error ? error : new Error(message);
      }
    });
    this.canvasChain = run.then(() => undefined, () => undefined);
    return run;
  }

  private async persistCapturedManual(
    workspaceId: string,
    editSession: ManualEditSession,
  ): Promise<void> {
    this.guard.assertActive(workspaceId, 'saving canvas edits');
    const workspaceState = this.get().state;
    if (!workspaceState || workspaceState.workspace.id !== workspaceId) {
      this.guard.fail('workspace changed while saving canvas edits');
    }
    if (editSession.baseVersionId !== workspaceState.workspace.versionId) {
      const message = '画布已更新到新版本，请再试一次刚才的改动。';
      this.set((current) => ({
        editSession: current.state?.workspace.id === workspaceId ? null : current.editSession,
        state: current.state?.workspace.id === workspaceId
          ? appendSystemError(current.state, message)
          : current.state,
      }));
      throw new Error(message);
    }
    const { graphOps, snapshot } = partitionManualEditOps(editSession.ops);
    let graphPersisted = false;
    try {
      if (graphOps.length > 0) {
        const next = await this.guard.run(workspaceId, 'saving canvas edits', () =>
          createManualWorkspaceProposal(
            workspaceId,
            manualEditInputFromSession({ ...editSession, ops: graphOps }),
          ));
        graphPersisted = true;
        this.set((current) => current.state?.workspace.id === workspaceId
          ? {
              state: next,
              status: 'ready',
              error: null,
              editSession: snapshot
                ? {
                    ...editSession,
                    baseVersionId: next.workspace.versionId,
                    ops: layoutOpsFromSnapshot(snapshot),
                  }
                : null,
            }
          : {});
      }

      if (snapshot && (this.get().canvas || !graphPersisted)) {
        await this.saveCanvas(snapshot);
      }

      this.set((current) => current.state?.workspace.id === workspaceId
        ? { editSession: null }
        : {});
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
      if (graphPersisted) {
        const message = error instanceof Error ? error.message : 'canvas snapshot request failed';
        this.set((latest) => latest.state?.workspace.id === workspaceId
          ? { editSession: null, canvasStatus: 'error', canvasError: message }
          : {});
        return;
      }
      const normalized = error instanceof Error ? error : new Error('canvas edit request failed');
      this.set((current) => ({
        editSession: current.state?.workspace.id === workspaceId ? null : current.editSession,
        state: current.state?.workspace.id === workspaceId
          ? appendSystemError(current.state, normalized.message)
          : current.state,
      }));
      throw normalized;
    }
  }
}
