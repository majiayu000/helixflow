import type { WorkbenchState } from '../types';

export type CanvasNodeArtifact = WorkbenchState['outputs'][number];

export function outputsByCanvasNode(
  outputs: WorkbenchState['outputs'] | undefined,
): Map<string, CanvasNodeArtifact[]> {
  const grouped = new Map<string, CanvasNodeArtifact[]>();
  const seen = new Set<string>();
  for (const output of outputs ?? []) {
    if (seen.has(output.id)) continue;
    seen.add(output.id);
    const nodeId = output.nodeId;
    if (!nodeId) continue;
    grouped.set(nodeId, [...(grouped.get(nodeId) ?? []), output]);
  }
  return grouped;
}
