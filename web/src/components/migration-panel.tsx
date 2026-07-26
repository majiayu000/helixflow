import { useEffect, useState } from 'react';
import { applyGraphMigration, runGraphMigrationDryRun } from '../api';
import type { MigrationDryRun } from '../types';

type MigrationPanelProps = {
  workspaceId: string;
  open: boolean;
  onClose: () => void;
  onApplied: () => void;
};

/// v1→v2 迁移面板（GH144）：dry-run 报告 + 显式 apply。needs_resolution 的
/// 节点逐条列出原因，永远不会静默应用。
export function MigrationPanel({ workspaceId, open, onClose, onApplied }: MigrationPanelProps) {
  const [dryRun, setDryRun] = useState<MigrationDryRun | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [appliedVersion, setAppliedVersion] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setDryRun(null);
    setError(null);
    setAppliedVersion(null);
    runGraphMigrationDryRun(workspaceId)
      .then((result) => {
        if (!cancelled) setDryRun(result);
      })
      .catch((cause) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : '迁移检查失败');
      });
    return () => {
      cancelled = true;
    };
  }, [open, workspaceId]);

  if (!open) return null;

  const apply = () => {
    setApplying(true);
    setError(null);
    applyGraphMigration(workspaceId)
      .then((result) => {
        setAppliedVersion(result.migratedVersionId ?? null);
        setApplying(false);
        onApplied();
      })
      .catch((cause) => {
        setApplying(false);
        setError(cause instanceof Error ? cause.message : '迁移失败');
      });
  };

  return (
    <div className="migration-panel" data-testid="migration-panel">
      <div className="migration-panel-head">
        <strong>图迁移（v1 → v2）</strong>
        <button className="p-close" onClick={onClose} type="button">
          x
        </button>
      </div>
      {error && <div className="migration-error">{error}</div>}
      {!dryRun && !error && <div className="migration-loading">检查中…</div>}
      {dryRun && <MigrationReportView dryRun={dryRun} />}
      {appliedVersion && (
        <div className="migration-done">已迁移，新版本 {appliedVersion.slice(0, 12)}…</div>
      )}
      {dryRun?.status === 'ready' && !appliedVersion && (
        <button
          className="migration-apply"
          disabled={applying}
          onClick={apply}
          type="button"
        >
          {applying ? '迁移中…' : '应用迁移（创建新版本，可回退）'}
        </button>
      )}
    </div>
  );
}

export function MigrationReportView({ dryRun }: { dryRun: MigrationDryRun }) {
  if (dryRun.status === 'alreadyMigrated') {
    return <div className="migration-status">当前版本已带语义层，无需迁移。</div>;
  }
  return (
    <div>
      <div className="migration-counts">
        可迁移 {dryRun.counts.mapped} · 结构节点 {dryRun.counts.structural} · 需人工处理{' '}
        {dryRun.counts.needsResolution}
      </div>
      {dryRun.status === 'needsResolution' && (
        <div className="migration-status migration-blocked">
          存在无法自动解析的节点，处理后重试：
        </div>
      )}
      <ul className="migration-nodes">
        {(dryRun.report?.nodes ?? []).map((node) => (
          <li key={node.nodeId}>
            <code>{node.nodeId}</code> {describeAction(node.action)}
          </li>
        ))}
      </ul>
    </div>
  );
}

type MigrationNodeAction = NonNullable<MigrationDryRun['report']>['nodes'][number]['action'];

function describeAction(action: MigrationNodeAction): string {
  if (action === 'structural') return '结构节点，无需语义';
  if ('mappedPolicy' in action) return `→ ${action.mappedPolicy.capabilityId}（默认策略）`;
  if ('mappedPinned' in action) {
    return `→ ${action.mappedPinned.capabilityId}（钉定 ${action.mappedPinned.modelId}）`;
  }
  if ('needsResolution' in action) return `⚠ ${action.needsResolution.reason}`;
  return `⚠ 多个候选模型：${action.needsUserChoice.candidates.join('、')}`;
}
