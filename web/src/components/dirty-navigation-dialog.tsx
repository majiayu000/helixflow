import { Icon } from '../icons';
import {
  pendingNavigationLabel,
  type DirtyNavigationDecision,
  type PendingNavigation,
} from '../dirty-navigation';

export function DirtyNavigationDialog({
  busy,
  target,
  onDecision,
}: {
  busy: boolean;
  target: PendingNavigation | null;
  onDecision: (decision: DirtyNavigationDecision) => void;
}) {
  if (!target) return null;
  return (
    <div className="confirm-layer" role="dialog" aria-modal="true" aria-label="未提交编辑">
      <div className="confirm">
        <div className="confirm-head">
          <span className="ic"><Icon n="alert" s={14} /></span>
          处理未提交编辑
        </div>
        <div className="confirm-list">
          <div className="confirm-item">
            <span className="k">目标</span>
            <span className="v">{pendingNavigationLabel(target)}</span>
          </div>
          <div className="confirm-item">
            <span className="k">当前编辑</span>
            <span className="v">提交后继续，或明确放弃；取消将留在当前 workspace。</span>
          </div>
        </div>
        <div className="confirm-foot">
          <button className="btn btn--quiet btn--sm" disabled={busy} onClick={() => onDecision('cancel')}>
            取消
          </button>
          <button className="btn btn--ghost btn--sm" disabled={busy} onClick={() => onDecision('discard')}>
            放弃并继续
          </button>
          <button className="btn btn--primary btn--sm" disabled={busy} onClick={() => onDecision('commit')}>
            提交并继续
          </button>
        </div>
      </div>
    </div>
  );
}
