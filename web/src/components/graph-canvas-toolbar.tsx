import { Icon } from '../icons';

type CanvasMode = 'view' | 'edit' | 'review';

type GraphCanvasToolbarProps = {
  activeMode: CanvasMode;
  clipboardStatus: string | null;
  connectionStatus: string | null;
  edgeCount: number;
  hasDirtyLayout: boolean;
  layoutSaving: boolean;
  layoutUpdateCount: number;
  nodeCount: number;
  onSaveLayout: () => void;
  pendingProposal: boolean;
  runStatusLabel: string;
  saveLayoutDisabled: boolean;
  setMode: (mode: CanvasMode) => void;
};

export function GraphCanvasToolbar({
  activeMode,
  clipboardStatus,
  connectionStatus,
  edgeCount,
  hasDirtyLayout,
  layoutSaving,
  layoutUpdateCount,
  nodeCount,
  onSaveLayout,
  pendingProposal,
  runStatusLabel,
  saveLayoutDisabled,
  setMode,
}: GraphCanvasToolbarProps) {
  return (
    <div className="canvas-toolbar" onPointerDown={(event) => event.stopPropagation()}>
      <div className="mode-seg">
        <button className={activeMode === 'view' ? 'on' : ''} onClick={() => setMode('view')}>
          查看
        </button>
        <button className={activeMode === 'edit' ? 'on' : ''} onClick={() => setMode('edit')}>
          编辑
        </button>
        <button className={activeMode === 'review' ? 'on' : ''} onClick={() => setMode('review')}>
          审阅
        </button>
      </div>
      <span className="canvas-pill">
        <Icon n="layers" s={13} c="var(--text-3)" />
        {nodeCount} 节点 · {edgeCount} 连线
      </span>
      <span className={pendingProposal ? 'pill pill--warn' : 'pill pill--off'}>
        <span className="led" />
        {pendingProposal ? '待确认的图变更 — 预览中' : runStatusLabel}
      </span>
      {connectionStatus && <span className="canvas-pill">{connectionStatus}</span>}
      {!pendingProposal && hasDirtyLayout && (
        <button className="layout-save" disabled={saveLayoutDisabled} onClick={onSaveLayout}>
          {layoutSaving ? '保存中' : `保存布局 · ${layoutUpdateCount}`}
        </button>
      )}
      {clipboardStatus && <span className="canvas-pill">{clipboardStatus}</span>}
    </div>
  );
}
