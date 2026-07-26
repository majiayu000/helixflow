import { useEffect, useRef, useState } from 'react';
import { applyVersionMigration, dryRunVersionMigration } from '../api';
import { Icon } from '../icons';
import type {
  RunStepState,
  VersionMigrationReport,
  WorkbenchState,
  WorkspaceSummary,
} from '../types';
import { formatTime } from './chat-pane';

type RunDockProps = {
  run: NonNullable<WorkbenchState['run']>;
};

type ConfirmModalProps = {
  confirmation: WorkbenchState['pendingConfirmation'];
  busy: boolean;
  onApprove: (id: string) => Promise<void>;
  onHold: (id: string) => Promise<void>;
};

type HistoryPanelProps = {
  history: WorkbenchState['history'];
  workspaces: WorkspaceSummary[];
  currentWorkspaceId: string;
  currentVersionId: string;
  currentConnectorId: string;
  busy: boolean;
  workspaceListError: string | null;
  open: boolean;
  onClose: () => void;
  onOpenWorkspace: (id: string) => void;
  onRestoreVersion: (id: string) => void;
  onMigrationApplied: (state: WorkbenchState) => void;
};

export function RunDock({ run }: RunDockProps) {
  if (!run.id && run.steps.length === 0) return null;
  const progress = runProgress(run.steps);
  const status = run.status;

  return (
    <div className="run-monitor">
      <div className="run-head">
        <span className={statusPillClass(status)}>
          <span className="led" />
          {statusLabel(status)}
        </span>
        <span className="title">{run.label}</span>
        <span className="time">
          {run.cost.actual || run.cost.estimate} {run.cost.currency}
        </span>
      </div>
      <div className="progress-track">
        <div className="progress-fill" style={{ width: `${progress}%` }} />
      </div>
      <div className="run-steps">
        {run.steps.map((step) => (
          <span className={`run-step ${stepClass(step.state)}`} key={step.nodeId}>
            {step.state === 'succeeded' && <Icon n="check" s={11} />}
            {step.state === 'running' && <span className="spin p-rotating" />}
            {step.state === 'failed' && <Icon n="alert" s={11} />}
            {step.title}
            {step.cached && <span className="run-step-cache">缓存</span>}
          </span>
        ))}
      </div>
    </div>
  );
}

export function OutputsStrip({
  outputs,
  busy,
  onSelect,
  onAccept,
  onReject,
}: {
  outputs: WorkbenchState['outputs'];
  busy: boolean;
  onSelect: (id: string) => void;
  onAccept: (id: string) => void;
  onReject: (id: string, rerun: boolean) => void;
}) {
  if (!outputs.length) return null;
  return (
    <div className="outputs">
      {outputs.map((output) => {
        const review = output.reviewState ?? 'pending';
        return (
          <div className={`output-item-wrap output-review--${review}`} key={output.id}>
            <button
              className="output-item"
              disabled={busy}
              onClick={() => onSelect(output.id)}
              type="button"
            >
              <div className={`output-thumb ${output.selected ? 'sel' : ''}`} data-kind={output.kind}>
                <Icon n={outputIcon(output.kind)} s={18} />
              </div>
              <div className="output-title">{output.title}</div>
            </button>
            <div className="output-review">
              {review === 'pending' ? (
                <>
                  <button
                    className="output-review-accept"
                    disabled={busy}
                    onClick={() => onAccept(output.id)}
                    title="接受该输出"
                    type="button"
                  >
                    <Icon n="check" s={12} />
                  </button>
                  <button
                    className="output-review-reject"
                    disabled={busy}
                    onClick={() => onReject(output.id, true)}
                    title="打回并重跑"
                    type="button"
                  >
                    <Icon n="undo" s={12} />
                  </button>
                </>
              ) : (
                <span className="output-review-state">
                  {review === 'accepted' ? '已接受' : '已打回'}
                </span>
              )}
            </div>
          </div>
        );
      })}
      <div className="outputs-note">{outputs.length} 个真实 artifact</div>
    </div>
  );
}

function outputIcon(kind: WorkbenchState['outputs'][number]['kind']): 'export' | 'image' | 'layers' | 'play' {
  if (kind === 'image') return 'image';
  if (kind === 'video') return 'play';
  if (kind === 'html' || kind === 'markdown' || kind === 'text') return 'export';
  return 'layers';
}

export function ConfirmModal({ confirmation, busy, onApprove, onHold }: ConfirmModalProps) {
  if (!confirmation) return null;
  return (
    <div className="confirm-layer">
      <div className="confirm">
        <div className="confirm-head">
          <span className="ic">
            <Icon n="warn" s={14} />
          </span>
          成本确认
        </div>
        <div className="confirm-list">
          <div className="confirm-item">
            <span className="k">请求</span>
            <span className="v">{confirmation.title}</span>
          </div>
          <div className="confirm-item">
            <span className="k">说明</span>
            <span className="v">{confirmation.summary}</span>
          </div>
          <div className="confirm-item">
            <span className="k">费用估算</span>
            <span className="v">
              {formatCostAmount(confirmation.cost.amount)} {confirmation.cost.currency}
            </span>
          </div>
          {confirmation.runCount ? (
            <div className="confirm-item">
              <span className="k">运行数量</span>
              <span className="v">{confirmation.runCount} 次运行</span>
            </div>
          ) : null}
          {confirmation.pendingChanges?.length ? (
            <div className="confirm-item confirm-item--stack">
              <span className="k">待变更</span>
              <div className="v confirm-changes">
                {confirmation.pendingChanges.map((change) => (
                  <span key={change}>{change}</span>
                ))}
              </div>
            </div>
          ) : null}
          {confirmation.interruptible !== undefined ? (
            <div className="confirm-item">
              <span className="k">中断</span>
              <span className="v">{confirmation.interruptible ? '执行期间可中断' : '不可中断'}</span>
            </div>
          ) : null}
          <div className="confirm-item">
            <span className="k">真实执行</span>
            <span className="v">超过阈值才需要确认，确认后调用运行服务</span>
          </div>
        </div>
        <div className="confirm-foot">
          <button className="btn btn--ghost btn--sm" disabled={busy} onClick={() => onHold(confirmation.id)}>
            取消
          </button>
          <button className="btn btn--primary btn--sm" disabled={busy} onClick={() => onApprove(confirmation.id)}>
            <Icon n="check" s={13} />
            确认运行
          </button>
        </div>
      </div>
    </div>
  );
}

function formatCostAmount(amount: number): string {
  if (amount === 0 || Math.abs(amount) >= 0.01) return amount.toFixed(2);
  if (Math.abs(amount) < 0.000001) return amount.toExponential(2);
  return amount.toFixed(6).replace(/0+$/, '').replace(/\.$/, '');
}

export function HistoryPanel({
  history,
  workspaces,
  currentWorkspaceId,
  currentVersionId,
  currentConnectorId,
  busy,
  workspaceListError,
  open,
  onClose,
  onOpenWorkspace,
  onRestoreVersion,
  onMigrationApplied,
}: HistoryPanelProps) {
  const [migrationReport, setMigrationReport] = useState<VersionMigrationReport | null>(null);
  const [migrationBusy, setMigrationBusy] = useState(false);
  const [migrationError, setMigrationError] = useState<string | null>(null);
  const [migrationSuccess, setMigrationSuccess] = useState<string | null>(null);
  const [confirmMigration, setConfirmMigration] = useState(false);
  const requestGeneration = useRef(0);
  const requestAbort = useRef<AbortController | null>(null);
  const operationId = useRef<string | null>(null);
  const expectedVersion = useRef<string | null>(null);
  useEffect(() => {
    const appliedVersionArrived = expectedVersion.current === currentVersionId;
    requestAbort.current?.abort();
    requestGeneration.current += 1;
    setMigrationReport(null);
    setMigrationError(null);
    if (!appliedVersionArrived) setMigrationSuccess(null);
    setMigrationBusy(false);
    setConfirmMigration(false);
    operationId.current = null;
    expectedVersion.current = null;
  }, [currentConnectorId, currentVersionId, currentWorkspaceId, open]);

  if (!open) return null;
  const inspectMigration = async () => {
    const generation = ++requestGeneration.current;
    requestAbort.current?.abort();
    const controller = new AbortController();
    requestAbort.current = controller;
    setMigrationBusy(true);
    setMigrationError(null);
    setConfirmMigration(false);
    try {
      const report = await dryRunVersionMigration(
        currentWorkspaceId,
        currentVersionId,
        controller.signal,
      );
      if (requestGeneration.current === generation) setMigrationReport(report);
    } catch (error) {
      if (requestGeneration.current === generation) {
        setMigrationError(error instanceof Error ? error.message : '迁移检查失败');
      }
    } finally {
      if (requestGeneration.current === generation) setMigrationBusy(false);
    }
  };
  const applyMigration = async () => {
    if (!migrationReport || migrationReport.status !== 'migratable') return;
    const generation = requestGeneration.current;
    const id = operationId.current ?? crypto.randomUUID();
    operationId.current = id;
    setMigrationBusy(true);
    setMigrationError(null);
    try {
      const result = await applyVersionMigration(currentWorkspaceId, currentVersionId, {
        operationId: id,
        reportHash: migrationReport.reportHash,
        sourceGraphHash: migrationReport.sourceGraphHash,
        catalogRevision: migrationReport.catalogRevision,
        workspaceConnectorId: migrationReport.workspaceConnectorId,
        migrationVersion: migrationReport.migrationVersion,
      });
      if (requestGeneration.current === generation) {
        expectedVersion.current = result.targetVersionId;
        setMigrationReport(null);
        setConfirmMigration(false);
        setMigrationSuccess(`已迁移到 ${result.targetVersionId}`);
        onMigrationApplied(result.workspaceState);
      }
    } catch (error) {
      if (requestGeneration.current === generation) {
        const message = error instanceof Error ? error.message : '迁移应用失败';
        setMigrationError(message);
        if (message === '迁移前提已变化，请重新检查') {
          setMigrationReport(null);
          setConfirmMigration(false);
          operationId.current = null;
        }
      }
    } finally {
      if (requestGeneration.current === generation) setMigrationBusy(false);
    }
  };
  return (
    <div className="history-panel-pop">
      <div className="history-head">
        历史记录
        <button className="p-close" onClick={onClose}>
          x
        </button>
      </div>
      <div className="history-body">
        <div className="history-section-title">对话历史</div>
        {workspaceListError ? (
          <div className="history-error">{workspaceListError}</div>
        ) : workspaces.length === 0 ? (
          <div className="empty-history">暂无对话历史</div>
        ) : (
          workspaces.map((workspace) => (
            <button
              className={`workspace-history-row ${
                workspace.id === currentWorkspaceId ? 'is-current' : ''
              }`}
              disabled={busy || workspace.id === currentWorkspaceId}
              key={workspace.id}
              onClick={() => onOpenWorkspace(workspace.id)}
            >
              <div>
                <div className="workspace-history-title">
                  {workspaceTitle(workspace)}
                </div>
                <div className="workspace-history-meta">
                  {workspace.messageCount} 条消息 · {workspace.versionId ?? '未生成版本'}
                </div>
              </div>
              <time>{formatTime(workspace.updatedAt)}</time>
            </button>
          ))
        )}
        <div className="history-section-title">版本与运行历史</div>
        <div className="history-row" data-kind="migration">
          <span className="history-dot" />
          <div>
            <div className="history-label">v1 → v2 版本迁移</div>
            <div aria-live="polite" className="history-summary">
              {migrationSummary(migrationReport, migrationError, migrationSuccess)}
            </div>
            {migrationReport?.nodes
              .filter((node) => node.code)
              .map((node) => (
                <div className="history-summary" key={node.nodeId}>
                  {node.nodeId}: {node.code}
                  {node.message ? `：${node.message}` : ''}
                  {node.candidates.length > 0 ? `（候选：${node.candidates.join('、')}）` : ''}
                </div>
              ))}
          </div>
          <div className="history-tail">
            {migrationReport?.status === 'migratable' ? (
              <button
                className="history-action"
                disabled={busy || migrationBusy || !migrationReport.applyEnabled}
                onClick={() => confirmMigration ? void applyMigration() : setConfirmMigration(true)}
              >
                {confirmMigration ? '确认迁移' : '迁移'}
              </button>
            ) : (
              <button
                className="history-action"
                disabled={busy || migrationBusy}
                onClick={() => void inspectMigration()}
              >
                {migrationBusy ? '检查中' : '检查'}
              </button>
            )}
          </div>
        </div>
        {history.length === 0 ? (
          <div className="empty-history">暂无版本或运行记录</div>
        ) : (
          history.map((item) => (
            <div className="history-row" data-kind={item.kind} key={item.id}>
              <span className="history-dot" />
              <div>
                <div className="history-label">{item.label}</div>
                <div className="history-summary">{item.summary}</div>
              </div>
              <div className="history-tail">
                <time>{formatTime(item.time)}</time>
                {item.kind === 'version' && (
                  <button
                    className="history-action"
                    disabled={busy || item.id === currentVersionId}
                    onClick={() => onRestoreVersion(item.id)}
                  >
                    {item.id === currentVersionId ? '当前' : '恢复'}
                  </button>
                )}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function migrationSummary(
  report: VersionMigrationReport | null,
  error: string | null,
  success: string | null,
): string {
  if (error) return error;
  if (success) return success;
  if (!report) return '先检查当前版本；只有服务端判定可迁移时才能应用。';
  if (report.status === 'migratable') {
    return report.applyEnabled ? '可以迁移；请再次确认。' : '可以迁移，但服务端尚未开放 apply。';
  }
  if (report.status === 'already_migrated') return '当前版本已经是 v2，无需迁移。';
  if (report.status === 'needs_resolution') return '存在需要人工选择的节点，暂不能迁移。';
  return `${report.code ?? 'MIGRATION_FAILED'}${report.message ? `：${report.message}` : ''}`;
}

function workspaceTitle(workspace: WorkspaceSummary): string {
  const message = workspace.firstMessage?.trim();
  if (message) return message;
  if (workspace.name.trim()) return workspace.name;
  return workspace.id;
}

function runProgress(steps: NonNullable<WorkbenchState['run']>['steps']): number {
  if (steps.length === 0) return 0;
  const done = steps.filter((step) => step.state === 'succeeded' || step.state === 'skipped').length;
  const running = steps.some((step) => step.state === 'running') ? 0.4 : 0;
  return Math.min(100, Math.round(((done + running) / steps.length) * 100));
}

function stepClass(state: RunStepState): string {
  if (state === 'succeeded') return 'done';
  if (state === 'running') return 'active';
  if (state === 'failed') return 'failed';
  return '';
}

function statusPillClass(status: NonNullable<WorkbenchState['run']>['status']): string {
  if (status === 'running') return 'pill pill--live';
  if (status === 'succeeded') return 'pill pill--ok';
  if (status === 'failed') return 'pill pill--danger';
  if (status === 'waiting_confirmation') return 'pill pill--warn';
  return 'pill pill--off';
}

function statusLabel(status: NonNullable<WorkbenchState['run']>['status']): string {
  if (status === 'running') return '运行中';
  if (status === 'succeeded') return '已完成';
  if (status === 'failed') return '失败';
  if (status === 'interrupted') return '已中断';
  if (status === 'waiting_confirmation') return '等待确认';
  if (status === 'estimating') return '估算中';
  return '未运行';
}
