import { Icon } from '../../icons';
import type { GraphNodeState } from '../../types';
import type { ViewState } from '../graph-canvas-navigation';
import type { WorkflowFlowInstance } from './types';
import type { CanvasPointerTool } from './use-canvas-tools';
import { setFlowViewportToNodes } from './use-viewport';

export function CanvasViewControls({
  activeTool,
  canEdit,
  instance,
  minimapOpen,
  nodes,
  onToolChange,
  onToggleMinimap,
  view,
  viewportSize,
}: {
  activeTool?: CanvasPointerTool;
  canEdit?: boolean;
  instance: WorkflowFlowInstance | null;
  minimapOpen?: boolean;
  nodes: GraphNodeState[];
  onToolChange?: (tool: CanvasPointerTool) => void;
  onToggleMinimap?: () => void;
  view: ViewState;
  viewportSize: { width: number; height: number };
}) {
  const nodeCount = nodes.length;
  const zoomPercent = Math.round(view.z * 100);
  return (
    <div className="flow-view-controls nodrag nopan" role="toolbar" aria-label="画布视图控制">
      {canEdit && onToolChange ? (
        <>
          <button
            aria-label="平移画布"
            aria-pressed={activeTool === 'pan'}
            className={activeTool === 'pan' ? 'is-active' : undefined}
            onClick={() => onToolChange('pan')}
            type="button"
          >
            <Icon n="hand" s={15} />
          </button>
          <button
            aria-label="框选节点"
            aria-pressed={activeTool === 'select'}
            className={activeTool === 'select' ? 'is-active' : undefined}
            onClick={() => onToolChange('select')}
            type="button"
          >
            <Icon n="grid" s={15} />
          </button>
          <span className="flow-view-divider" />
        </>
      ) : null}
      <button aria-label="缩小画布" onClick={() => void instance?.zoomOut({ duration: 160 })}>
        −
      </button>
      <button
        aria-label="重置画布缩放"
        className="flow-view-zoom"
        onClick={() => void instance?.zoomTo(1, { duration: 180 })}
      >
        {zoomPercent}%
      </button>
      <button aria-label="放大画布" onClick={() => void instance?.zoomIn({ duration: 160 })}>
        +
      </button>
      <span className="flow-view-divider" />
      <button
        aria-label="适应全部节点"
        disabled={nodeCount === 0}
        onClick={() => void setFlowViewportToNodes(instance, nodes, viewportSize)}
      >
        <Icon n="layers" s={15} />
      </button>
      {onToggleMinimap ? (
        <button
          aria-label="切换小地图"
          aria-pressed={minimapOpen}
          className={minimapOpen ? 'is-active' : undefined}
          onClick={onToggleMinimap}
          type="button"
        >
          <Icon n="layers" s={13} />
        </button>
      ) : null}
      <span className="flow-node-count">{nodeCount.toLocaleString()} nodes</span>
    </div>
  );
}

export function CanvasStatusToast({
  editStatus,
  pendingProposal,
}: {
  editStatus: string | null;
  pendingProposal: boolean;
}) {
  const message = pendingProposal ? '待确认的图变更 · 预览中' : editStatus;
  if (!message) return null;
  return <div className="canvas-status-toast" role="status" aria-live="polite">{message}</div>;
}
