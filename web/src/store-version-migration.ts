import { create } from 'zustand';
import {
  applyVersionMigration,
  dryRunVersionMigration,
  VersionMigrationApiError,
} from './api-version-migration';
import type { VersionMigrationReport } from './version-migration-types';
import type { WorkbenchState } from './types';

export type VersionMigrationContext = {
  workspaceId: string;
  versionId: string;
  connectorId: string;
};

type PendingApply = {
  context: VersionMigrationContext;
  operationId: string;
  report: VersionMigrationReport;
  status: 'applying' | 'unknown';
};

type CompletedApply = {
  context: VersionMigrationContext;
  targetVersionId: string;
  workspaceState: WorkbenchState;
};

export type VersionMigrationPhase =
  | 'idle'
  | 'checking'
  | 'ready'
  | 'applying'
  | 'unknown'
  | 'pending_elsewhere'
  | 'conflict'
  | 'failed'
  | 'success';

type VersionMigrationStore = {
  context: VersionMigrationContext | null;
  phase: VersionMigrationPhase;
  report: VersionMigrationReport | null;
  error: string | null;
  successVersionId: string | null;
  pendingApply: PendingApply | null;
  completedApply: CompletedApply | null;
  setContext: (context: VersionMigrationContext | null) => void;
  inspect: () => Promise<void>;
  apply: (onApplied: (state: WorkbenchState) => void) => Promise<void>;
  hydrateCompleted: (onApplied: (state: WorkbenchState) => void) => void;
};

let checkController: AbortController | null = null;

export const useVersionMigrationStore = create<VersionMigrationStore>((set, get) => ({
  context: null,
  phase: 'idle',
  report: null,
  error: null,
  successVersionId: null,
  pendingApply: null,
  completedApply: null,

  setContext: (context) => {
    checkController?.abort();
    checkController = null;
    const { pendingApply, completedApply, successVersionId } = get();
    if (!context) {
      set({ context, phase: 'idle', report: null, error: null });
      return;
    }
    if (pendingApply) {
      const matching = sameContext(context, pendingApply.context);
      set({
        context,
        phase: matching ? pendingApply.status : 'pending_elsewhere',
        report: matching ? pendingApply.report : null,
        error: matching && pendingApply.status === 'unknown'
          ? '上次迁移结果未知；请使用同一 operation ID 重试确认。'
          : null,
      });
      return;
    }
    if (completedApply && sameContext(context, completedApply.context)) {
      set({
        context,
        phase: 'success',
        report: null,
        error: null,
        successVersionId: completedApply.targetVersionId,
      });
      return;
    }
    const succeeded = successVersionId === context.versionId;
    set({
      context,
      phase: succeeded ? 'success' : 'idle',
      report: null,
      error: null,
      successVersionId: succeeded ? successVersionId : null,
    });
  },

  inspect: async () => {
    const context = get().context;
    if (!context || get().pendingApply) return;
    checkController?.abort();
    const controller = new AbortController();
    checkController = controller;
    const key = contextKey(context);
    set({ phase: 'checking', report: null, error: null, successVersionId: null });
    try {
      const report = await dryRunVersionMigration(
        context.workspaceId,
        context.versionId,
        controller.signal,
      );
      if (get().context && contextKey(get().context!) === key) {
        set({ phase: 'ready', report, error: null });
      }
    } catch (error) {
      if (controller.signal.aborted) return;
      if (get().context && contextKey(get().context!) === key) {
        set({
          phase: 'failed',
          error: error instanceof Error ? error.message : '迁移检查失败',
        });
      }
    }
  },

  apply: async (onApplied) => {
    const state = get();
    const context = state.context;
    if (!context || state.phase === 'pending_elsewhere') return;
    const pending = state.pendingApply;
    const matchingPending = pending && sameContext(context, pending.context) ? pending : null;
    const report = matchingPending?.report ?? state.report;
    if (!report || report.status !== 'migratable' || !report.applyEnabled) return;
    const operationId = matchingPending?.operationId ?? createVersionMigrationOperationId();
    const applyState: PendingApply = { context, operationId, report, status: 'applying' };
    const key = contextKey(context);
    set({ phase: 'applying', pendingApply: applyState, error: null });
    try {
      const result = await applyVersionMigration(context.workspaceId, context.versionId, {
        operationId,
        reportHash: report.reportHash,
        sourceGraphHash: report.sourceGraphHash,
        catalogRevision: report.catalogRevision,
        workspaceConnectorId: report.workspaceConnectorId,
        migrationVersion: report.migrationVersion,
      });
      const visible = get().context && contextKey(get().context!) === key;
      set({
        pendingApply: null,
        phase: visible ? 'success' : 'idle',
        report: null,
        error: null,
        successVersionId: result.targetVersionId,
        completedApply: visible
          ? null
          : { context, targetVersionId: result.targetVersionId, workspaceState: result.workspaceState },
      });
      if (visible) onApplied(result.workspaceState);
    } catch (error) {
      const visible = get().context && contextKey(get().context!) === key;
      if (error instanceof VersionMigrationApiError && error.status === 409) {
        set({
          pendingApply: null,
          phase: visible ? 'conflict' : 'idle',
          report: null,
          error: visible ? error.message : null,
        });
        return;
      }
      if (
        error instanceof VersionMigrationApiError
        && (
          error.status < 500
          || (error.status === 503 && error.code === 'MIGRATION_APPLY_DISABLED')
        )
      ) {
        set({
          pendingApply: null,
          phase: visible ? 'failed' : 'idle',
          report: null,
          error: visible ? error.message : null,
        });
        return;
      }
      const unknown: PendingApply = { ...applyState, status: 'unknown' };
      set({
        pendingApply: unknown,
        phase: visible ? 'unknown' : 'pending_elsewhere',
        error: visible
          ? '迁移结果未知；请使用同一 operation ID 重试确认。'
          : get().error,
      });
    }
  },

  hydrateCompleted: (onApplied) => {
    const { completedApply, context } = get();
    if (!completedApply || !context || !sameContext(context, completedApply.context)) return;
    set({ completedApply: null });
    onApplied(completedApply.workspaceState);
  },
}));

export function createVersionMigrationOperationId(): string {
  const randomUuid = globalThis.crypto?.randomUUID?.();
  if (randomUuid) return `migration_${randomUuid}`;
  const suffix = Math.random().toString(36).slice(2, 12);
  return `migration_${Date.now()}_${suffix}`;
}

function sameContext(left: VersionMigrationContext, right: VersionMigrationContext): boolean {
  return contextKey(left) === contextKey(right);
}

function contextKey(context: VersionMigrationContext): string {
  return `${context.workspaceId}\u0000${context.versionId}\u0000${context.connectorId}`;
}
