import { HistoryIcon, Icon } from '../icons';
import type { ConnectionStatus } from '../api';
import type { WorkbenchState } from '../types';

type TopBarProps = {
  state: WorkbenchState;
  connection: ConnectionStatus;
  historyOpen: boolean;
  agentRunDisabled: boolean;
  exportDisabled: boolean;
  undoDisabled: boolean;
  runDisabled: boolean;
  running: boolean;
  busy: boolean;
  onHistory: () => void;
  onNewWorkspace: () => void;
  onAgentRun: () => void;
  onExport: () => void;
  onUndo: () => void;
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
  running,
  busy,
  onHistory,
  onNewWorkspace,
  onAgentRun,
  onExport,
  onUndo,
  onQueue,
}: TopBarProps) {
  const nodeCount = state.graph.nodes.length;
  const defaultProvider =
    state.providers.runtimeProviders.find(
      (provider) => provider.id === state.providers.defaultProvider,
    ) ??
    state.providers.runtimeProviders[0] ??
    null;
  const providerOk = Boolean(defaultProvider?.enabled && defaultProvider.status === 'healthy');
  const providerLabel = defaultProvider?.label ?? 'Provider state missing';
  const providerMessage = defaultProvider?.message ?? providerStatusLabel(defaultProvider);

  return (
    <div className="wb-top">
      <div className="brand">
        <span className="brand-mark">C</span>
        ComfyUI Agent
      </div>
      <span className="divider-v" />
      <div className="workflow-tabs">
        <span className="workflow-tab workflow-tab--active">{state.workspace.name}</span>
        <button className="workflow-tab" disabled={busy} onClick={onNewWorkspace}>
          + 新建
        </button>
      </div>
      <div className="top-actions">
        <div className="endpoint-box">
          <span className="endpoint">
            <Icon n="lock" s={11} />
            {providerLabel}
          </span>
          <span className="pill pill--ok">
            <span className="led" />
            {nodeCount} 节点
          </span>
          <span className={connection === 'live' ? 'pill pill--live' : 'pill pill--off'}>
            <span className="led" />
            Codex {connectionLabel(connection)}
          </span>
          <span className={providerOk ? 'pill pill--live' : 'pill pill--off'}>
            <span className="led" />
            {providerStatusLabel(defaultProvider)}
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
        <button className="btn btn--soft btn--sm" disabled={agentRunDisabled || busy} onClick={onAgentRun}>
          <Icon n="spark" s={13} fill />
          Agent 运行
        </button>
        <button
          className={running ? 'btn btn--danger btn--sm' : 'btn btn--primary btn--sm'}
          disabled={runDisabled || (!running && busy)}
          title={
            running
              ? '中断当前运行'
              : runDisabled && !providerOk
              ? providerMessage
              : runDisabled
                ? '等待当前请求完成'
                : '提交当前工作流运行'
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
