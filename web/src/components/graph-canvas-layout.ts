import type { GraphNodeState, LayoutPositionUpdate } from '../types';

export type PositionDrafts = Record<string, { x: number; y: number }>;

export type DragNodeStart = {
  id: string;
  x: number;
  y: number;
};

export function applyPositionDrafts(
  nodes: GraphNodeState[],
  drafts: PositionDrafts,
): GraphNodeState[] {
  return nodes.map((node) => {
    const draft = drafts[node.id];
    if (!draft) return node;
    return {
      ...node,
      position: { x: draft.x, y: draft.y },
    };
  });
}

export function moveNodeDrafts(
  starts: DragNodeStart[],
  delta: { x: number; y: number },
): PositionDrafts {
  return Object.fromEntries(
    starts.map((node) => [
      node.id,
      {
        x: node.x + delta.x,
        y: node.y + delta.y,
      },
    ]),
  );
}

export function positionUpdatesFromDrafts(
  baseNodes: GraphNodeState[],
  drafts: PositionDrafts,
): LayoutPositionUpdate[] {
  return baseNodes
    .flatMap((node) => {
      const draft = drafts[node.id];
      if (!draft || !Number.isFinite(draft.x) || !Number.isFinite(draft.y)) return [];
      if (draft.x === node.position.x && draft.y === node.position.y) return [];
      return [{ id: node.id, x: draft.x, y: draft.y }];
    })
    .sort((left, right) => left.id.localeCompare(right.id));
}

export function selectionForNodePointer(
  current: ReadonlySet<string>,
  nodeId: string,
  additive: boolean,
): Set<string> {
  if (!additive) {
    return current.has(nodeId) ? new Set(current) : new Set([nodeId]);
  }
  const next = new Set(current);
  if (next.has(nodeId)) {
    next.delete(nodeId);
  } else {
    next.add(nodeId);
  }
  return next;
}
