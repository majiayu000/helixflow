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
  const zoomPercent = Math.round(view.z * 100);
  const setZoom = (z: number) => {
    setView((current) => ({ ...current, z: clampZoom(z) }));
  };

  return (
    <div className="zoom-ctl" onPointerDown={(event) => event.stopPropagation()}>
      <button aria-label="缩小画布" onClick={() => setZoom(view.z - 0.1)}>
        -
      </button>
      <input
        aria-label="缩放画布"
        max="200"
        min="25"
        onChange={(event) => setZoom(Number(event.currentTarget.value) / 100)}
        type="range"
        value={zoomPercent}
      />
      <button aria-label="放大画布" onClick={() => setZoom(view.z + 0.1)}>
        +
      </button>
      <span>{zoomPercent}%</span>
      <button aria-label="重置画布缩放" onClick={() => setZoom(1)}>
        ⊕
      </button>
    </div>
  );
}

function clampZoom(value: number): number {
  return Math.min(2, Math.max(0.25, value));
}
