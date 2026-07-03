import { Icon } from '../icons';
import type { ViewState } from './graph-canvas-navigation';

export function EmptyCanvas() {
  return (
    <div className="empty-canvas">
      <div className="empty-card">
        <div className="empty-icon">
          <Icon n="layers" s={22} />
        </div>
        <div className="empty-title">空白工作流</div>
        <div className="empty-sub">先描述要设计的结果；需要自动化时再生成 workflow。</div>
      </div>
    </div>
  );
}

export function ZoomControls({
  setView,
  view,
}: {
  setView: (updater: (current: ViewState) => ViewState) => void;
  view: ViewState;
}) {
  return (
    <div className="zoom-ctl" onPointerDown={(event) => event.stopPropagation()}>
      <button onClick={() => setView((current) => ({ ...current, z: current.z - 0.1 }))}>
        -
      </button>
      <span>{Math.round(view.z * 100)}%</span>
      <button onClick={() => setView((current) => ({ ...current, z: current.z + 0.1 }))}>
        +
      </button>
    </div>
  );
}
