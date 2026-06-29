import { HistoryIcon, Icon } from '../icons';
import type { ConnectionStatus } from '../api';
import type { WorkbenchState } from '../types';

type TopBarProps = {
  state: WorkbenchState;
  connection: ConnectionStatus;
  historyOpen: boolean;
  agentRunDisabled: boolean;
  runDisabled: boolean;
  running: boolean;
  busy: boolean;
  onHistory: () => void;
  onNewWorkspace: () => void;
  onAgentRun: () => void;
  onQueue: () => void;
};

export function TopBar({
  state,
  connection,
  historyOpen,
  agentRunDisabled,
  runDisabled,
  running,
  busy,
  onHistory,
  onNewWorkspace,
  onAgentRun,
  onQueue,
}: TopBarProps) {
  const nodeCount = state.graph.nodes.length;
  const defaultProvider =
    state.providers.providers.find((provider) => provider.id === state.providers.defaultProvider) ??
    state.providers.providers[0] ??
    null;
  const providerOk = Boolean(defaultProvider?.enabled);

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
            {defaultProvider?.label ?? 'API provider'}
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
            {providerOk ? 'Atlas 已配置' : 'Atlas 未配置'}
          </span>
        </div>
        <button className="ibtn ibtn--icon" title="撤销上次应用" disabled>
          <Icon n="undo" />
        </button>
        <button
          className={`ibtn ibtn--icon ${historyOpen ? 'p-on' : ''}`}
          title="版本与运行历史"
          onClick={onHistory}
        >
          <HistoryIcon />
        </button>
        <button className="ibtn ibtn--icon" title="导出 API workflow JSON" disabled>
          <Icon n="export" />
        </button>
        <span className="divider-v" />
        <button className="btn btn--soft btn--sm" disabled={agentRunDisabled || busy} onClick={onAgentRun}>
          <Icon n="spark" s={13} fill />
          Agent 运行
        </button>
        <button
          className={running ? 'btn btn--danger btn--sm' : 'btn btn--primary btn--sm'}
          disabled={runDisabled || busy}
          title={
            runDisabled && !providerOk
              ? (defaultProvider?.health.message ?? 'Atlas provider 未配置')
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
