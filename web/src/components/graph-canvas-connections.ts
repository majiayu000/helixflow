import type {
  GraphNodeState,
  ManualProposalInput,
  NodeDefinition,
  WorkbenchState,
} from '../types';
import {
  GRAPH_NODE_HEAD_HEIGHT,
  GRAPH_NODE_ROW_HEIGHT,
  GRAPH_NODE_WIDTH,
} from './graph-canvas-navigation';
import type { Point } from './graph-canvas-selection';

export type PortDirection = 'input' | 'output';
export type ConnectionPort = {
  nodeId: string;
  port: string;
  type: string;
};
export type InputPortState = 'compatible' | 'incompatible' | 'occupied';
export type PortHighlight = InputPortState | 'source';

export function portKey(nodeId: string, direction: PortDirection, port: string): string {
  return `${nodeId}:${direction}:${port}`;
}

export function portTypeMatches(outputType: string, inputType: string): boolean {
  return normalizePortType(outputType) === normalizePortType(inputType);
}

export function edgeTypeForPort(type: string): string {
  return normalizePortType(type);
}

export function buildPortHighlights(
  nodes: GraphNodeState[],
  definitionByType: Map<string, NodeDefinition>,
  source: ConnectionPort,
  edges: WorkbenchState['graph']['edges'],
): Map<string, PortHighlight> {
  const highlights = new Map<string, PortHighlight>();
  highlights.set(portKey(source.nodeId, 'output', source.port), 'source');

  for (const node of nodes) {
    const definition = definitionByType.get(node.nodeType);
    for (const input of definition?.inputs ?? []) {
      const compatible = portTypeMatches(source.type, input.type);
      const existing = findInputConnection(edges, { nodeId: node.id, port: input.name });
      highlights.set(
        portKey(node.id, 'input', input.name),
        compatible ? (existing ? 'occupied' : 'compatible') : 'incompatible',
      );
    }
  }

  return highlights;
}

export function findInputConnection(
  edges: WorkbenchState['graph']['edges'],
  target: Pick<ConnectionPort, 'nodeId' | 'port'>,
): WorkbenchState['graph']['edges'][number] | undefined {
  return edges.find((edge) => edge.to.nodeId === target.nodeId && edge.to.port === target.port);
}

export function buildConnectionProposalInput(input: {
  baseVersionId: string;
  source: ConnectionPort;
  target: ConnectionPort;
  existingEdge?: WorkbenchState['graph']['edges'][number];
}): ManualProposalInput | null {
  if (!portTypeMatches(input.source.type, input.target.type)) return null;

  const nextEdge = {
    from: [input.source.nodeId, input.source.port] as [string, string],
    to: [input.target.nodeId, input.target.port] as [string, string],
    edge_type: edgeTypeForPort(input.source.type),
  };
  const existingEdge = input.existingEdge;
  if (existingEdge && sameConnection(existingEdge, nextEdge)) return null;

  const ops: ManualProposalInput['ops'] = existingEdge
    ? [edgeToRemoveOp(existingEdge), { op: 'add_edge', ...nextEdge }]
    : [{ op: 'add_edge', ...nextEdge }];

  return {
    baseVersionId: input.baseVersionId,
    label: existingEdge
      ? `替换 ${input.target.nodeId}.${input.target.port} 输入连线`
      : `连接 ${input.source.nodeId}.${input.source.port} -> ${input.target.nodeId}.${input.target.port}`,
    ops,
  };
}

export function edgeToRemoveOp(
  edge: WorkbenchState['graph']['edges'][number],
): Extract<ManualProposalInput['ops'][number], { op: 'remove_edge' }> {
  return {
    op: 'remove_edge',
    from: [edge.from.nodeId, edge.from.port],
    to: [edge.to.nodeId, edge.to.port],
    edge_type: edge.kind,
  };
}

export function connectionPath(from: Point, to: Point): string {
  const dx = Math.max(40, Math.abs(to.x - from.x) * 0.5);
  return `M ${from.x} ${from.y} C ${from.x + dx} ${from.y}, ${to.x - dx} ${to.y}, ${to.x} ${to.y}`;
}

export function portAnchorPoint(
  node: GraphNodeState,
  direction: PortDirection,
  index: number,
): Point {
  const x = direction === 'output' ? node.position.x + GRAPH_NODE_WIDTH : node.position.x;
  return {
    x,
    y: node.position.y + GRAPH_NODE_HEAD_HEIGHT + GRAPH_NODE_ROW_HEIGHT + index * 22,
  };
}

function normalizePortType(type: string): string {
  return type.trim().toLowerCase();
}

function sameConnection(
  edge: WorkbenchState['graph']['edges'][number],
  next: { from: [string, string]; to: [string, string]; edge_type: string },
): boolean {
  return (
    edge.from.nodeId === next.from[0] &&
    edge.from.port === next.from[1] &&
    edge.to.nodeId === next.to[0] &&
    edge.to.port === next.to[1] &&
    edge.kind === next.edge_type
  );
}
