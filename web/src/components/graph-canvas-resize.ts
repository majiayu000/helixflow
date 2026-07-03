import { useCallback, useRef, type Dispatch, type PointerEvent, type SetStateAction } from 'react';
import type { GraphNodeState, NodeSizeUpdate } from '../types';
import { buildResizeNodeEditInput } from '../workbench-edit-session';
import {
  GRAPH_NODE_MAX_HEIGHT,
  GRAPH_NODE_MAX_WIDTH,
  GRAPH_NODE_MIN_HEIGHT,
  GRAPH_NODE_MIN_WIDTH,
  graphNodeHeight,
  graphNodeWidth,
} from './graph-canvas-navigation';

export type SizeDrafts = Record<string, { width: number; height: number }>;

export type ResizeNodeStart = {
  id: string;
  width: number;
  height: number;
};

type NodeResizeState = {
  pointerId: number;
  sx: number;
  sy: number;
  start: ResizeNodeStart;
};

type ResizeControllerInput = {
  connectionDisabled: boolean;
  onCreateProposal?: (input: NonNullable<ReturnType<typeof buildResizeNodeEditInput>>) => Promise<void>;
  pendingProposal: boolean;
  setConnectionStatus: Dispatch<SetStateAction<string | null>>;
  setDraftSizes: Dispatch<SetStateAction<SizeDrafts>>;
  setSelectedIds: Dispatch<SetStateAction<Set<string>>>;
  sourceNodes: GraphNodeState[];
  versionId: string;
  viewZoom: number;
};

export function useNodeResizeController({
  connectionDisabled,
  onCreateProposal,
  pendingProposal,
  setConnectionStatus,
  setDraftSizes,
  setSelectedIds,
  sourceNodes,
  versionId,
  viewZoom,
}: ResizeControllerInput) {
  const nodeResize = useRef<NodeResizeState | null>(null);

  const resetNodeResize = useCallback(() => {
    nodeResize.current = null;
  }, []);

  const startNodeResize = useCallback(
    (node: GraphNodeState, event: PointerEvent<HTMLSpanElement>) => {
      if (event.button !== 0 || pendingProposal || connectionDisabled) return;
      event.preventDefault();
      event.stopPropagation();
      setSelectedIds(new Set([node.id]));
      event.currentTarget.setPointerCapture(event.pointerId);
      nodeResize.current = {
        pointerId: event.pointerId,
        sx: event.clientX,
        sy: event.clientY,
        start: { id: node.id, width: graphNodeWidth(node), height: graphNodeHeight(node) },
      };
    },
    [connectionDisabled, pendingProposal, setSelectedIds],
  );

  const handleNodeResizeMove = useCallback(
    (event: PointerEvent<HTMLSpanElement>) => {
      const current = nodeResize.current;
      if (!current || current.pointerId !== event.pointerId) return;
      event.preventDefault();
      event.stopPropagation();
      setDraftSizes((drafts) => ({
        ...drafts,
        ...resizeNodeDraft(current.start, {
          width: (event.clientX - current.sx) / viewZoom,
          height: (event.clientY - current.sy) / viewZoom,
        }),
      }));
    },
    [setDraftSizes, viewZoom],
  );

  const stopNodeResize = useCallback(
    (event: PointerEvent<HTMLSpanElement>) => {
      const current = nodeResize.current;
      if (!current || current.pointerId !== event.pointerId) return;
      event.preventDefault();
      event.stopPropagation();
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
      const resized = resizeNodeDraft(current.start, {
        width: (event.clientX - current.sx) / viewZoom,
        height: (event.clientY - current.sy) / viewZoom,
      });
      const updates = sizeUpdatesFromDrafts(sourceNodes, resized);
      const editInput = buildResizeNodeEditInput(versionId, updates);
      if (editInput && onCreateProposal && !pendingProposal) {
        void onCreateProposal(editInput)
          .then(() => {
            setDraftSizes({});
            setConnectionStatus(`已加入编辑会话 · ${updates.length} 个 resize`);
          })
          .catch((error) => {
            setDraftSizes({});
            setConnectionStatus(error instanceof Error ? error.message : '调整节点大小失败');
          });
      } else {
        setDraftSizes({});
      }
      nodeResize.current = null;
    },
    [onCreateProposal, pendingProposal, setConnectionStatus, setDraftSizes, sourceNodes, versionId, viewZoom],
  );

  return { handleNodeResizeMove, resetNodeResize, startNodeResize, stopNodeResize };
}

export function applySizeDrafts(nodes: GraphNodeState[], drafts: SizeDrafts): GraphNodeState[] {
  return nodes.map((node) => {
    const draft = drafts[node.id];
    if (!draft) return node;
    return {
      ...node,
      size: { width: draft.width, height: draft.height },
    };
  });
}

export function resizeNodeDraft(
  start: ResizeNodeStart,
  delta: { width: number; height: number },
): SizeDrafts {
  return {
    [start.id]: {
      width: clampNodeWidth(start.width + delta.width),
      height: clampNodeHeight(start.height + delta.height),
    },
  };
}

export function sizeUpdatesFromDrafts(
  baseNodes: GraphNodeState[],
  drafts: SizeDrafts,
): NodeSizeUpdate[] {
  return baseNodes
    .flatMap((node) => {
      const draft = drafts[node.id];
      if (!draft || !Number.isFinite(draft.width) || !Number.isFinite(draft.height)) return [];
      const currentWidth = graphNodeWidth(node);
      const currentHeight = graphNodeHeight(node);
      if (draft.width === currentWidth && draft.height === currentHeight) return [];
      return [{ id: node.id, width: draft.width, height: draft.height }];
    })
    .sort((left, right) => left.id.localeCompare(right.id));
}

function clampNodeWidth(width: number): number {
  if (!Number.isFinite(width)) return GRAPH_NODE_MIN_WIDTH;
  return Math.min(GRAPH_NODE_MAX_WIDTH, Math.max(GRAPH_NODE_MIN_WIDTH, width));
}

function clampNodeHeight(height: number): number {
  if (!Number.isFinite(height)) return GRAPH_NODE_MIN_HEIGHT;
  return Math.min(GRAPH_NODE_MAX_HEIGHT, Math.max(GRAPH_NODE_MIN_HEIGHT, height));
}
