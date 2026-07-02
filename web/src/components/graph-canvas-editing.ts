import { z } from 'zod';
import type {
  ManualProposalInput,
  NodeDefinition,
  WorkbenchState,
  WorkflowGraph,
} from '../types';
import type { Point } from './graph-canvas-selection';

const ClipboardNodeSchema = z
  .object({
    node_type: z.string(),
    title: z.string(),
    params: z.unknown(),
    pos: z.tuple([z.number(), z.number()]),
  })
  .strict();

const ClipboardEdgeSchema = z
  .object({
    from: z.tuple([z.string(), z.string()]),
    to: z.tuple([z.string(), z.string()]),
    edge_type: z.string(),
  })
  .strict();

const ClipboardPayloadSchema = z
  .object({
    app: z.literal('helixflow'),
    kind: z.literal('canvas_selection'),
    schema_version: z.literal(1),
    sourceVersionId: z.string(),
    nodes: z.record(z.string(), ClipboardNodeSchema),
    edges: z.array(ClipboardEdgeSchema),
  })
  .strict();

export type CanvasClipboardPayload = z.infer<typeof ClipboardPayloadSchema>;

export function buildAddNodeProposalInput(input: {
  baseVersionId: string;
  definition: NodeDefinition;
  existingNodeIds: Iterable<string>;
  position: Point;
  suffix?: string;
}): ManualProposalInput {
  const id = uniqueNodeId(
    input.existingNodeIds,
    `${slugFromType(input.definition.type)}_${input.suffix ?? randomSuffix()}`,
  );
  return {
    baseVersionId: input.baseVersionId,
    label: `添加节点 ${id}`,
    ops: [{
      op: 'add_node',
      id,
      node_type: input.definition.type,
      title: input.definition.title,
      params: defaultParamsForDefinition(input.definition),
      pos: [input.position.x, input.position.y],
    }],
  };
}

export function buildDeleteNodesProposalInput(input: {
  baseVersionId: string;
  selectedNodeIds: Iterable<string>;
}): ManualProposalInput | null {
  const ids = [...input.selectedNodeIds].sort();
  if (ids.length === 0) return null;
  return {
    baseVersionId: input.baseVersionId,
    label: `删除 ${ids.length} 个节点`,
    ops: ids.map((id) => ({ op: 'remove_node', id })),
  };
}

export function buildSelectionClipboardText(input: {
  sourceVersionId: string;
  nodes: WorkbenchState['graph']['nodes'];
  edges: WorkbenchState['graph']['edges'];
  workflowGraph?: WorkflowGraph;
}): string {
  const selectedIds = new Set(input.nodes.map((node) => node.id));
  const payload: CanvasClipboardPayload = {
    app: 'helixflow',
    kind: 'canvas_selection',
    schema_version: 1,
    sourceVersionId: input.sourceVersionId,
    nodes: Object.fromEntries(
      input.nodes
        .slice()
        .sort((left, right) => left.id.localeCompare(right.id))
        .map((node) => {
          const workflowNode = input.workflowGraph?.nodes[node.id];
          return [
            node.id,
            {
              node_type: node.nodeType,
              title: node.title,
              params: workflowNode?.params ?? {},
              pos: [node.position.x, node.position.y] as [number, number],
            },
          ];
        }),
    ),
    edges: input.edges
      .filter((edge) => selectedIds.has(edge.from.nodeId) && selectedIds.has(edge.to.nodeId))
      .map((edge) => ({
        from: [edge.from.nodeId, edge.from.port],
        to: [edge.to.nodeId, edge.to.port],
        edge_type: edge.kind,
      })),
  };
  const labels = input.nodes.map((node) => `${node.id}: ${node.title}`).join('\n');
  return `Helixflow selection (${input.nodes.length} nodes)\n${labels}\n\n${JSON.stringify(payload, null, 2)}`;
}

export function buildPasteProposalInput(input: {
  baseVersionId: string;
  existingNodeIds: Iterable<string>;
  text: string;
  position: Point;
}): ManualProposalInput | null {
  const payload = parseClipboardPayload(input.text);
  if (!payload) return null;
  const entries = Object.entries(payload.nodes).sort(([left], [right]) => left.localeCompare(right));
  if (entries.length === 0) return null;

  const usedIds = new Set(input.existingNodeIds);
  const idMap = new Map<string, string>();
  for (const [oldId, node] of entries) {
    const nextId = uniqueNodeId(usedIds, slugFromType(oldId || node.node_type));
    usedIds.add(nextId);
    idMap.set(oldId, nextId);
  }

  const minX = Math.min(...entries.map(([, node]) => node.pos[0]));
  const minY = Math.min(...entries.map(([, node]) => node.pos[1]));
  const addNodeOps = entries.map(([oldId, node]) => ({
    op: 'add_node' as const,
    id: idMap.get(oldId)!,
    node_type: node.node_type,
    title: node.title,
    params: node.params,
    pos: [
      input.position.x + node.pos[0] - minX,
      input.position.y + node.pos[1] - minY,
    ] as [number, number],
  }));
  const addEdgeOps = payload.edges.flatMap((edge) => {
    const fromId = idMap.get(edge.from[0]);
    const toId = idMap.get(edge.to[0]);
    if (!fromId || !toId) return [];
    return [{
      op: 'add_edge' as const,
      from: [fromId, edge.from[1]] as [string, string],
      to: [toId, edge.to[1]] as [string, string],
      edge_type: edge.edge_type,
    }];
  });

  return {
    baseVersionId: input.baseVersionId,
    label: `粘贴 ${entries.length} 个节点`,
    ops: [...addNodeOps, ...addEdgeOps],
  };
}

export function defaultParamsForDefinition(definition: NodeDefinition): Record<string, unknown> {
  return Object.fromEntries(
    definition.params_schema.required.map((key) => [
      key,
      defaultValueForParam(definition.params_schema.properties[key]),
    ]),
  );
}

export function uniqueNodeId(existingNodeIds: Iterable<string>, base: string): string {
  const existing = new Set(existingNodeIds);
  const safeBase = sanitizeNodeId(base) || 'node';
  if (!existing.has(safeBase)) return safeBase;
  for (let index = 2; index < 10000; index += 1) {
    const candidate = `${safeBase}_${index}`;
    if (!existing.has(candidate)) return candidate;
  }
  throw new Error(`cannot generate unique id for ${safeBase}`);
}

function parseClipboardPayload(text: string): CanvasClipboardPayload | null {
  const jsonText = text.slice(Math.max(0, text.indexOf('{')));
  if (!jsonText.trim()) return null;
  try {
    return ClipboardPayloadSchema.parse(JSON.parse(jsonText));
  } catch {
    return null;
  }
}

function slugFromType(value: string): string {
  return sanitizeNodeId(value.replace(/\./g, '_'));
}

function sanitizeNodeId(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_]+/g, '_')
    .replace(/^_+|_+$/g, '');
}

function randomSuffix(): string {
  if (typeof crypto !== 'undefined' && crypto.getRandomValues) {
    const bytes = crypto.getRandomValues(new Uint8Array(3));
    return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
  }
  return Math.random().toString(36).slice(2, 8) || 'node';
}

function defaultValueForParam(
  spec: NodeDefinition['params_schema']['properties'][string] | undefined,
): unknown {
  if (spec?.enum_values[0] !== undefined) return spec.enum_values[0];
  if (spec?.type === 'integer' || spec?.type === 'number') return spec.minimum ?? 0;
  if (spec?.type === 'boolean') return false;
  return '';
}
