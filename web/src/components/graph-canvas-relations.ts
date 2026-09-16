import type { WorkbenchState } from '../types';

export function relatedHighlight(
  activeNodeId: string | null | undefined,
  edges: WorkbenchState['graph']['edges'],
): { nodeIds: Set<string>; edgeIds: Set<string> } {
  const nodeIds = new Set<string>();
  const edgeIds = new Set<string>();
  if (!activeNodeId) return { nodeIds, edgeIds };
  nodeIds.add(activeNodeId);
  for (const edge of edges) {
    if (edge.from.nodeId !== activeNodeId && edge.to.nodeId !== activeNodeId) continue;
    edgeIds.add(edge.id);
    nodeIds.add(edge.from.nodeId);
    nodeIds.add(edge.to.nodeId);
  }
  return { nodeIds, edgeIds };
}
