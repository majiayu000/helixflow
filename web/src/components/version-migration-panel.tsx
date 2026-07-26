import { useEffect, useState } from 'react';
import { useVersionMigrationStore } from '../store-version-migration';
import type { VersionMigrationReport } from '../version-migration-types';
import type { WorkbenchState } from '../types';

type VersionMigrationPanelProps = {
  workspaceId: string;
  versionId: string;
  connectorId: string;
  busy: boolean;
  open: boolean;
  onApplied: (state: WorkbenchState) => void;
};

export function VersionMigrationPanel({
  workspaceId,
  versionId,
  connectorId,
  busy,
  open,
  onApplied,
}: VersionMigrationPanelProps) {
  const phase = useVersionMigrationStore((state) => state.phase);
  const report = useVersionMigrationStore((state) => state.report);
  const error = useVersionMigrationStore((state) => state.error);
  const successVersionId = useVersionMigrationStore((state) => state.successVersionId);
  const completedApply = useVersionMigrationStore((state) => state.completedApply);
  const setContext = useVersionMigrationStore((state) => state.setContext);
  const inspect = useVersionMigrationStore((state) => state.inspect);
  const apply = useVersionMigrationStore((state) => state.apply);
  const hydrateCompleted = useVersionMigrationStore((state) => state.hydrateCompleted);
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    setContext(open ? { workspaceId, versionId, connectorId } : null);
    setConfirming(false);
  }, [connectorId, open, setContext, versionId, workspaceId]);

  useEffect(() => {
    if (completedApply) hydrateCompleted(onApplied);
  }, [
    completedApply,
    connectorId,
    hydrateCompleted,
    onApplied,
    open,
    versionId,
    workspaceId,
  ]);

  const migratable = report?.status === 'migratable';
  const retryUnknown = phase === 'unknown';
  const actionDisabled =
    busy ||
    phase === 'checking' ||
    phase === 'applying' ||
    phase === 'pending_elsewhere' ||
    (migratable && !report.applyEnabled);

  return (
    <div className="history-row" data-kind="migration">
      <span className="history-dot" />
      <div>
        <div className="history-label">v1 → v2 版本迁移</div>
        <div aria-live="polite" className="history-summary">
          {migrationSummary(phase, report, error, successVersionId)}
        </div>
        {report?.nodes
          .filter((node) => node.code)
          .map((node) => (
            <div aria-label={`迁移节点 ${node.nodeId}`} className="history-summary" key={node.nodeId}>
              {node.nodeId}: {node.code}
              {node.message ? `：${node.message}` : ''}
              {node.candidates.length > 0 ? `（候选：${node.candidates.join('、')}）` : ''}
            </div>
          ))}
      </div>
      <div className="history-tail">
        <button
          aria-label={actionLabel(phase, migratable, confirming)}
          className="history-action"
          disabled={actionDisabled}
          onClick={() => {
            if (retryUnknown || (migratable && confirming)) void apply(onApplied);
            else if (migratable) setConfirming(true);
            else void inspect();
          }}
        >
          {actionLabel(phase, migratable, confirming)}
        </button>
      </div>
    </div>
  );
}

function actionLabel(
  phase: ReturnType<typeof useVersionMigrationStore.getState>['phase'],
  migratable: boolean,
  confirming: boolean,
): string {
  if (phase === 'checking') return '检查中';
  if (phase === 'applying') return '迁移中';
  if (phase === 'unknown') return '重试确认';
  if (phase === 'pending_elsewhere') return '返回原工作区';
  if (migratable) return confirming ? '确认迁移' : '迁移';
  return '检查';
}

function migrationSummary(
  phase: ReturnType<typeof useVersionMigrationStore.getState>['phase'],
  report: VersionMigrationReport | null,
  error: string | null,
  successVersionId: string | null,
): string {
  if (phase === 'success') return `已迁移到 ${successVersionId ?? 'v2'}`;
  if (phase === 'pending_elsewhere') return '另一个工作区的迁移结果待确认，请先返回原工作区。';
  if (error) return error;
  if (!report) return '先检查当前版本；只有服务端判定可迁移时才能应用。';
  const mapped = report.nodes.filter((node) => node.action.startsWith('mapped_')).length;
  const structural = report.nodes.filter((node) => node.action === 'structural').length;
  const unresolved = report.nodes.filter((node) => node.action.startsWith('needs_')).length;
  const preview =
    `连接器 ${report.workspaceConnectorId}；影响：映射 ${mapped}、结构 ${structural}、待处理 ${unresolved}。`;
  if (report.status === 'migratable') {
    return report.applyEnabled
      ? `${preview} 可以迁移；请再次确认。`
      : `${preview} 可以迁移，但服务端尚未开放 apply。`;
  }
  if (report.status === 'already_migrated') return `${preview} 当前版本已经是 v2，无需迁移。`;
  if (report.status === 'needs_resolution') {
    return `${preview} 存在需要人工选择的节点，暂不能迁移。`;
  }
  return `${preview} ${report.code ?? 'MIGRATION_FAILED'}${report.message ? `：${report.message}` : ''}`;
}
