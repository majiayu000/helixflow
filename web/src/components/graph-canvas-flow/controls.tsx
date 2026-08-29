import { Icon } from '../../icons';
import type { GraphNodeState } from '../../types';
import type { ViewState } from '../graph-canvas-navigation';
import type { WorkflowFlowInstance } from './types';
import { setFlowViewportToNodes } from './use-viewport';

export function CanvasViewControls({
  instance,
  nodes,
  view,
  viewportSize,
}: {
  instance: WorkflowFlowInstance | null;
  nodes: GraphNodeState[];
  view: ViewState;
  viewportSize: { width: number; height: number };
}) {
  const nodeCount = nodes.length;
  const zoomPercent = Math.round(view.z * 100);
  return (
    <div className="flow-view-controls nodrag nopan" role="toolbar" aria-label="画布视图控制">
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
