import { useCallback, useEffect, useRef, type Dispatch, type PointerEvent, type SetStateAction } from 'react';
import type { GraphNodeState } from '../types';
import { buildMoveNodeEditInput } from '../workbench-edit-session';
import {
  moveNodeDrafts,
  positionUpdatesFromDrafts,
  selectionForNodePointer,
  type PositionDrafts,
} from './graph-canvas-layout';
import type { GraphCanvasProps, NodeDragState } from './graph-canvas-types';

type NodeDragControllerInput = {
  baseNodes: GraphNodeState[];
  canMove: boolean;
  nodeById: Map<string, GraphNodeState>;
  onCreateProposal: GraphCanvasProps['onCreateProposal'];
  pendingProposal: boolean;
  selectedIds: Set<string>;
  setDraftPositions: Dispatch<SetStateAction<PositionDrafts>>;
  setSelectedIds: Dispatch<SetStateAction<Set<string>>>;
  setStatus: Dispatch<SetStateAction<string | null>>;
  versionId: string;
  viewZoom: number;
};

export function useCanvasNodeDragController({
  baseNodes,
  canMove,
  nodeById,
  onCreateProposal,
  pendingProposal,
  selectedIds,
  setDraftPositions,
  setSelectedIds,
  setStatus,
  versionId,
  viewZoom,
}: NodeDragControllerInput) {
  const drag = useRef<NodeDragState | null>(null);
  const reset = useCallback(() => {
    drag.current = null;
  }, []);

  useEffect(() => {
    if (!canMove) drag.current = null;
  }, [canMove]);

  const start = (node: GraphNodeState, event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.stopPropagation();
    const additive = event.shiftKey || event.metaKey || event.ctrlKey;
    const nextSelection = selectionForNodePointer(selectedIds, node.id, additive);
    setSelectedIds(nextSelection);
    if (!canMove) return;

    const starts = [...nextSelection]
      .map((nodeId) => nodeById.get(nodeId))
      .filter((item): item is GraphNodeState => Boolean(item))
      .map((item) => ({ id: item.id, x: item.position.x, y: item.position.y }));
    if (starts.length === 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      pointerId: event.pointerId,
      sx: event.clientX,
      sy: event.clientY,
      starts,
    };
  };

  const move = (event: PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    if (!canMove) {
      drag.current = null;
      return;
    }
    event.stopPropagation();
    const moved = moveNodeDrafts(current.starts, {
      x: (event.clientX - current.sx) / viewZoom,
      y: (event.clientY - current.sy) / viewZoom,
    });
    setDraftPositions((drafts) => ({ ...drafts, ...moved }));
  };

  const stop = (event: PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    if (!canMove) {
      drag.current = null;
      return;
    }
    event.stopPropagation();
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    const moved = moveNodeDrafts(current.starts, {
      x: (event.clientX - current.sx) / viewZoom,
      y: (event.clientY - current.sy) / viewZoom,
    });
    const updates = positionUpdatesFromDrafts(baseNodes, moved);
    const editInput = buildMoveNodeEditInput(versionId, updates);
    if (editInput && onCreateProposal && !pendingProposal) {
      void onCreateProposal(editInput)
        .then(() => {
          setStatus(`已移动 ${updates.length} 个节点`);
        })
        .catch((error) => {
          setStatus(error instanceof Error ? error.message : '移动节点失败');
        })
        .finally(() => {
          setDraftPositions({});
        });
    }
    drag.current = null;
  };

  return { move, reset, start, stop };
}
