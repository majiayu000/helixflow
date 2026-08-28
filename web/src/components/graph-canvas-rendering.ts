import type { GraphNodeState, RunStepState, WorkbenchState } from '../types';
import {
  GRAPH_NODE_HEAD_HEIGHT,
  GRAPH_NODE_ROW_HEIGHT,
  graphNodeHeight,
  graphNodeWidth,
  type ViewportSize,
  type ViewState,
} from './graph-canvas-navigation';

export type DiffState = 'add' | 'upd' | null;

export type Param = {
  key: string;
  value: string;
};

export function buildNodeMap(nodes: GraphNodeState[]): Map<string, GraphNodeState> {
  return new Map(nodes.map((node) => [node.id, node] as const));
}

export function buildRunStepStateMap(
  steps: NonNullable<WorkbenchState['run']>['steps'],
): Map<string, RunStepState> {
  return new Map(steps.map((step) => [step.nodeId, step.state] as const));
}

export function buildEdgeSignatureSet(edges: WorkbenchState['graph']['edges']): Set<string> {
  return new Set(edges.map((edge) => edgeSignature(edge)));
}

export function buildComparableNodeMap(nodes: GraphNodeState[]): Map<string, string> {
  return new Map(nodes.map((node) => [node.id, comparableNode(node)] as const));
}

export function edgesForViewport(
  nodes: GraphNodeState[],
  edges: WorkbenchState['graph']['edges'],
  view: ViewState,
  viewport: ViewportSize,
  densityLimit = 2_000,
): WorkbenchState['graph']['edges'] {
  if (edges.length <= densityLimit) return edges;
  const overscan = 480;
  const visibleTargets = new Set(
    nodes
      .filter((node) => nodeIntersectsViewport(node, view, viewport, overscan))
      .map((node) => node.id),
  );
  return edges.filter((edge) => visibleTargets.has(edge.to.nodeId));
}

/**
 * Returns the dense-graph nodes React Flow must know about for the current
 * viewport. `null` means the caller should preserve the complete node array.
 * Edge endpoints stay in the slice so React Flow can still resolve visible
 * connections whose source sits outside the overscanned viewport.
 */
export function nodeIdsForViewport(
  nodes: GraphNodeState[],
  visibleEdges: WorkbenchState['graph']['edges'],
  view: ViewState,
  viewport: ViewportSize,
  densityLimit = 2_000,
): Set<string> | null {
  if (nodes.length <= densityLimit) return null;
  const overscan = 480;
  const nodeIds = new Set(
    nodes
      .filter((node) => nodeIntersectsViewport(node, view, viewport, overscan))
      .map((node) => node.id),
  );
  for (const edge of visibleEdges) {
    nodeIds.add(edge.from.nodeId);
    nodeIds.add(edge.to.nodeId);
  }
  return nodeIds;
}

export function nodeDiffState(
  node: GraphNodeState,
  baseComparable: string | undefined,
  hasProposal: boolean,
): DiffState {
  if (!hasProposal) return null;
  if (!baseComparable) return 'add';
  return comparableNode(node) === baseComparable ? null : 'upd';
}

export function edgeSignature(edge: WorkbenchState['graph']['edges'][number]): string {
  return `${edge.from.nodeId}:${edge.from.port}>${edge.to.nodeId}:${edge.to.port}:${edge.kind}`;
}

export function edgePath(from: GraphNodeState, to: GraphNodeState): string {
  const x1 = from.position.x + graphNodeWidth(from);
  const y1 = from.position.y + GRAPH_NODE_HEAD_HEIGHT + GRAPH_NODE_ROW_HEIGHT;
  const x2 = to.position.x;
  const y2 = to.position.y + GRAPH_NODE_HEAD_HEIGHT + GRAPH_NODE_ROW_HEIGHT;
  const dx = Math.max(40, Math.abs(x2 - x1) * 0.5);
  return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}

export function paramsFromSummary(summary: string): Param[] {
  if (!summary || summary === '{}') return [];
  try {
    const parsed = JSON.parse(summary) as Record<string, unknown>;
    return Object.entries(parsed).map(([key, value]) => ({
      key,
      value: typeof value === 'string' ? value : JSON.stringify(value),
    }));
  } catch {
    return [{ key: 'summary', value: summary }];
  }
}

export function categorySwatch(category: string): string {
  const key = category.toLowerCase();
  if (key.includes('input')) return 'var(--t-image)';
  if (key.includes('text')) return 'var(--t-cond)';
  if (key.includes('video')) return 'var(--t-clip)';
  if (key.includes('image')) return 'var(--t-image)';
  if (key.includes('output')) return 'var(--green)';
  if (key.includes('mock')) return 'var(--amber)';
  return 'var(--accent)';
}

export function runStatusLabel(status: NonNullable<WorkbenchState['run']>['status']): string {
  if (status === 'running') return '运行中';
  if (status === 'succeeded') return '已完成';
  if (status === 'failed') return '失败';
  if (status === 'interrupted') return '已中断';
  if (status === 'estimating') return '估算中';
  return '未运行';
}

function comparableNode(node: GraphNodeState): string {
  return JSON.stringify({
    id: node.id,
    nodeType: node.nodeType,
    title: node.title,
    category: node.category,
    position: node.position,
    size: node.size,
    provider: node.provider,
    summary: node.summary,
  });
}

function nodeIntersectsViewport(
  node: GraphNodeState,
  view: ViewState,
  viewport: ViewportSize,
  overscan: number,
): boolean {
  const left = node.position.x * view.z + view.x;
  const top = node.position.y * view.z + view.y;
  const right = left + graphNodeWidth(node) * view.z;
  const bottom = top + graphNodeHeight(node) * view.z;
  return (
    right >= -overscan &&
    bottom >= -overscan &&
    left <= viewport.width + overscan &&
    top <= viewport.height + overscan
  );
}
