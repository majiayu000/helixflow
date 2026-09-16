import { isCanvasCardType } from '../grid-split';
import type {
  GraphNodeState,
  ManualProposalInput,
  NodeDefinition,
  WorkbenchState,
} from '../types';
import {
  GRAPH_NODE_HEAD_HEIGHT,
  GRAPH_NODE_ROW_HEIGHT,
  graphNodeWidth,
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

export const MEDIA_REF_PORT = 'in';

type LineagePorts = Pick<NodeDefinition, 'inputs' | 'outputs'>;

function port(
  name: string,
  type: NodeDefinition['inputs'][number]['type'],
  required: boolean,
  cardinality: NonNullable<NodeDefinition['inputs'][number]['cardinality']> = 'one',
): NodeDefinition['inputs'][number] {
  return { name, type, required, cardinality };
}

export function lineagePortDefinition(nodeType: string): LineagePorts | null {
  switch (nodeType) {
    case 'input.text':
      return {
        inputs: [port(MEDIA_REF_PORT, 'TEXT', false, 'many')],
        outputs: [port('text', 'TEXT', true)],
      };
    case 'input.image':
      return {
        inputs: [port(MEDIA_REF_PORT, 'IMAGE', false, 'many')],
        outputs: [port('image', 'IMAGE', true)],
      };
    case 'image.generate':
      return {
        inputs: [
          port('prompt', 'TEXT', true),
          port(MEDIA_REF_PORT, 'IMAGE', false, 'many'),
        ],
        outputs: [port('image', 'IMAGE', true)],
      };
    case 'image.edit':
      return {
        inputs: [
          port('image', 'IMAGE', true),
          port('prompt', 'TEXT', true),
          port(MEDIA_REF_PORT, 'IMAGE', false, 'many'),
        ],
        outputs: [port('image', 'IMAGE', true)],
      };
    case 'input.video':
      return {
        inputs: [port(MEDIA_REF_PORT, 'VIDEO', false, 'many')],
        outputs: [port('video', 'VIDEO', true)],
      };
    case 'input.audio':
      return {
        inputs: [port(MEDIA_REF_PORT, 'AUDIO', false, 'many')],
        outputs: [port('audio', 'AUDIO', true)],
      };
    default:
      return null;
  }
}

export function canvasCardFallbackPorts(nodeType: string): {
  inputs: Array<{ name: string; type: string }>;
  outputs: Array<{ name: string; type: string }>;
} | null {
  if (!isCanvasCardType(nodeType)) return null;
  const definition = lineagePortDefinition(nodeType);
  if (!definition) return null;
  return {
    inputs: definition.inputs.map((item) => ({ name: item.name, type: item.type })),
    outputs: definition.outputs.map((item) => ({ name: item.name, type: item.type })),
  };
}

export function lineageConnection(
  sourceNodeType: string,
  targetNodeType: string,
): { sourcePort: string; targetPort: string; type: string } {
  const match = matchingConnectionPorts(
    lineagePortDefinition(sourceNodeType) ?? undefined,
    lineagePortDefinition(targetNodeType) ?? undefined,
  );
  if (!match) {
    throw new Error('连线失败：这两张卡没有可接的端口');
  }
  return match;
}

export function matchingConnectionPorts(
  sourceDefinition: LineagePorts | undefined,
  targetDefinition: LineagePorts | undefined,
): { sourcePort: string; targetPort: string; type: string } | null {
  const outputs = sourceDefinition?.outputs ?? [];
  const inputs = targetDefinition?.inputs ?? [];
  const ref = inputs.find((input) => input.name === MEDIA_REF_PORT);
  const executionInputs = inputs.filter((input) => input !== ref);
  for (const output of outputs) {
    const input = executionInputs.find((port) => portTypeMatches(output.type, port.type));
    if (input) return { sourcePort: output.name, targetPort: input.name, type: output.type };
  }
  for (const output of outputs) {
    if (ref && portTypeMatches(output.type, ref.type)) {
      return { sourcePort: output.name, targetPort: ref.name, type: output.type };
    }
  }
  if (ref && outputs[0]) {
    return { sourcePort: outputs[0].name, targetPort: ref.name, type: outputs[0].type };
  }
  return null;
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
      const compatible = portTypeMatches(source.type, input.type)
        || (input.name === MEDIA_REF_PORT && input.cardinality === 'many' && !input.required);
      const existing = findInputConnection(edges, { nodeId: node.id, port: input.name });
      const occupied = Boolean(existing) && input.cardinality !== 'many';
      highlights.set(
        portKey(node.id, 'input', input.name),
        compatible ? (occupied ? 'occupied' : 'compatible') : 'incompatible',
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
  fanIn?: boolean;
}): ManualProposalInput | null {
  if (
    !portTypeMatches(input.source.type, input.target.type)
    && input.target.port !== MEDIA_REF_PORT
  ) {
    return null;
  }

  const nextEdge = {
    from: [input.source.nodeId, input.source.port] as [string, string],
    to: [input.target.nodeId, input.target.port] as [string, string],
    edge_type: edgeTypeForPort(input.source.type),
  };
  const existingEdge = input.fanIn ? undefined : input.existingEdge;
  if (input.existingEdge && sameConnection(input.existingEdge, nextEdge)) return null;

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
  const x = direction === 'output' ? node.position.x + graphNodeWidth(node) : node.position.x;
  return {
    x,
    y: node.position.y + GRAPH_NODE_HEAD_HEIGHT + GRAPH_NODE_ROW_HEIGHT + index * 22,
  };
}

function normalizePortType(type: string): string {
  return type.trim().toLowerCase();
}

export type ReferenceBag = {
  images: number;
  videos: number;
  audios: number;
};

export type IncomingReference = {
  fromNodeId: string;
  fromPort: string;
  toPort: string;
  kind: string;
};

export function incomingReferences(
  nodeId: string,
  edges: WorkbenchState['graph']['edges'],
): IncomingReference[] {
  return edges
    .filter((edge) => edge.to.nodeId === nodeId)
    .filter((edge) => {
      const port = edge.to.port.toLowerCase();
      const kind = edge.kind.toLowerCase();
      return (
        port === 'image' ||
        port === 'video' ||
        port === 'audio' ||
        kind === 'image' ||
        kind === 'video' ||
        kind === 'audio'
      );
    })
    .map((edge) => ({
      fromNodeId: edge.from.nodeId,
      fromPort: edge.from.port,
      toPort: edge.to.port,
      kind: edge.kind,
    }));
}

export function incomingReferenceBag(
  nodeId: string,
  edges: WorkbenchState['graph']['edges'],
): ReferenceBag {
  const bag: ReferenceBag = { images: 0, videos: 0, audios: 0 };
  for (const ref of incomingReferences(nodeId, edges)) {
    const key = ref.toPort.toLowerCase() || ref.kind.toLowerCase();
    if (key === 'image') bag.images += 1;
    else if (key === 'video') bag.videos += 1;
    else if (key === 'audio') bag.audios += 1;
  }
  return bag;
}

export function referenceBagLabel(bag: ReferenceBag): string | null {
  const parts: string[] = [];
  if (bag.images > 0) parts.push(`${bag.images} 图`);
  if (bag.videos > 0) parts.push(`${bag.videos} 视频`);
  if (bag.audios > 0) parts.push(`${bag.audios} 音频`);
  if (parts.length === 0) return null;
  return `参考袋 ${parts.join(' · ')}`;
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
