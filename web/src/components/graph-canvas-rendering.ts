import type { GraphNodeState, RunStepState, WorkbenchState } from '../types';
import {
  GRAPH_NODE_HEAD_HEIGHT,
  GRAPH_NODE_ROW_HEIGHT,
  graphNodeWidth,
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
