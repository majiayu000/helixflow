import { HistoryIcon, Icon } from '../icons';
import type { ConnectionStatus } from '../api';
import type { WorkbenchState } from '../types';
import {
  queueLockReasonLabel,
  queueLockReasonTitle,
  type QueueLockReason,
} from '../workbench-edit-session';

type TopBarProps = {
  state: WorkbenchState;
  connection: ConnectionStatus;
  historyOpen: boolean;
  agentRunDisabled: boolean;
  exportDisabled: boolean;
  undoDisabled: boolean;
  runDisabled: boolean;
  queueLockReason: QueueLockReason;
  forceRerun: boolean;
  running: boolean;
  busy: boolean;
  onHistory: () => void;
  onNewWorkspace: () => void;
  onAgentRun: () => void;
  onCommitEdits?: () => void;
  onExport: () => void;
  onUndo: () => void;
  onProviderSelect: (providerId: string) => void;
  onForceRerunChange: (enabled: boolean) => void;
  onQueue: () => void;
};

export function TopBar({
  state,
  connection,
  historyOpen,
  agentRunDisabled,
  exportDisabled,
  undoDisabled,
  runDisabled,
  queueLockReason,
  forceRerun,
  running,
  busy,
  onHistory,
  onNewWorkspace,
  onAgentRun,
  onCommitEdits,
  onExport,
  onUndo,
  onProviderSelect,
  onForceRerunChange,
  onQueue,
}: TopBarProps) {
  const nodeCount = state.graph.nodes.length;
  const selectedProviderId = state.providers.selectedProvider ?? state.providers.defaultProvider;
  const selectedProvider =
    state.providers.runtimeProviders.find(
      (provider) => provider.id === selectedProviderId,
    ) ??
    state.providers.runtimeProviders[0] ??
    null;
  const providerOk = Boolean(selectedProvider?.enabled && selectedProvider.status === 'healthy');
  const providerLabel = selectedProvider?.label ?? selectedProviderId;
  const providerMessage = selectedProvider?.message ?? providerStatusLabel(selectedProvider);
  const versionNumber = versionOrdinal(state);
  const dirtyEditCount = queueLockReason.kind === 'dirty_edits' ? queueLockReason.count : 0;
  const nextVersionLabel = `v${versionNumber + (dirtyEditCount > 0 ? 1 : 0)}`;

  return (
    <div className="wb-top">
      <div className="top-left">
        <div className="brand" aria-label="helixflow">
          <span className="brand-mark" aria-hidden="true">
            <span />
          </span>
          <strong>helixflow</strong>
        </div>
        <span className="divider-v" />
        <div className="project-chip" title={state.workspace.name}>
          <strong>{workspaceTitle(state.workspace.name)}</strong>
          <span>v{versionNumber}</span>
        </div>
        {dirtyEditCount > 0 ? (
          <span className="edit-state edit-state--dirty">EDITING · {dirtyEditCount} CHANGES</span>
        ) : (
          <span className="edit-state">READY · v{versionNumber}</span>
        )}
        <button className="new-workspace-link" disabled={busy} onClick={onNewWorkspace}>
          + 新建
        </button>
      </div>
      <div className="top-actions">
        <div className="status-strip">
          <span className="endpoint">
            <Icon n="lock" s={11} />
            <select
              aria-label="Runtime provider"
              className="provider-select"
              disabled={busy || state.providers.runtimeProviders.length === 0}
              onChange={(event) => onProviderSelect(event.target.value)}
              title={providerMessage}
              value={selectedProviderId}
            >
              {state.providers.runtimeProviders.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.label} · {providerStatusLabel(provider)}
                </option>
              ))}
            </select>
          </span>
          <span className="pill pill--ok">
            <span className="led" />
            {nodeCount} 节点
          </span>
          {queueLockReason.kind !== 'none' && queueLockReason.kind !== 'dirty_edits' && !running && (
            <span className="pill pill--warn">
              <span className="led" />
              {queueLockReasonLabel(queueLockReason)}
            </span>
          )}
          <span className={connection === 'live' ? 'pill pill--live' : 'pill pill--off'}>
            <span className="led" />
            Codex {connectionLabel(connection)}
          </span>
          <span className={providerOk ? 'pill pill--live' : 'pill pill--off'}>
            <span className="led" />
            {providerLabel} · {providerStatusLabel(selectedProvider)}
          </span>
        </div>
        <button
          className="ibtn ibtn--icon"
          title="撤销上次应用"
          disabled={undoDisabled}
          onClick={onUndo}
        >
          <Icon n="undo" />
        </button>
        <button
          className={`ibtn ibtn--icon ${historyOpen ? 'p-on' : ''}`}
          title="版本与运行历史"
          onClick={onHistory}
        >
          <HistoryIcon />
        </button>
        <button
          className="ibtn ibtn--icon"
          title="导出 API workflow JSON"
          disabled={exportDisabled}
          onClick={onExport}
        >
          <Icon n="export" />
        </button>
        <span className="divider-v" />
        <button className="btn btn--soft btn--sm top-agent-run" disabled={agentRunDisabled || busy} onClick={onAgentRun}>
          <Icon n="spark" s={13} fill />
          Agent 运行
        </button>
        <label className="force-rerun-toggle" title="本次运行绕过节点缓存">
          <input
            checked={forceRerun}
            disabled={busy || running}
            onChange={(event) => onForceRerunChange(event.target.checked)}
            type="checkbox"
          />
          强制重跑
        </label>
        {dirtyEditCount > 0 && onCommitEdits && (
          <button className="btn btn--commit btn--sm" disabled={busy} onClick={onCommitEdits}>
            ✓ 提交编辑 → {nextVersionLabel}
          </button>
        )}
        <button
          className={running ? 'btn btn--danger btn--sm' : 'btn btn--queue btn--sm'}
          disabled={runDisabled || (!running && busy)}
          title={
            running
              ? '中断当前运行'
              : queueLockReasonTitle(queueLockReason)
          }
          onClick={onQueue}
        >
          <Icon n={running ? 'stop' : 'play'} s={13} fill />
          {running ? '中断' : '运行 Queue'}
        </button>
      </div>
    </div>
  );
}

function workspaceTitle(name: string): string {
  return name
    .replace(/^Helixflow Workspace\s*[-·]\s*/i, '')
    .trim() || 'Untitled Workspace';
}

function versionOrdinal(state: WorkbenchState): number {
  const versionCount = state.history.filter((item) => item.kind === 'version').length;
  return Math.max(1, versionCount);
}

function connectionLabel(connection: ConnectionStatus): string {
  if (connection === 'live') return '在线';
  if (connection === 'connecting') return '连接中';
  return '离线';
}

function providerStatusLabel(
  provider: WorkbenchState['providers']['runtimeProviders'][number] | null,
): string {
  if (!provider) return 'Provider 状态缺失';
  if (provider.kind === 'local_test' && provider.enabled && provider.status === 'healthy') {
    return '本地测试';
  }
  if (provider.enabled && provider.status === 'healthy') return '可用';
  if (provider.status === 'missing') return 'Provider 状态缺失';
  return '不可用';
}
