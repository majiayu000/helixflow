import { z } from 'zod';
import { isMediaNodeType, isVisualMediaCardType, type GridSplitTilePlacement } from '../grid-split';
import { matchingConnectionPorts } from './graph-canvas-connections';
import { imageCanvasToolLabel, type ImageCanvasToolKind } from '../image-canvas-tools';
import type {
  GraphNodeState,
  ManualProposalInput,
  NodeDefinition,
  WorkbenchState,
  WorkflowGraph,
} from '../types';
import { MEDIA_CARD_HEIGHT, MEDIA_CARD_WIDTH } from './graph-canvas-navigation';
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
  connectFrom?: {
    nodeId: string;
    definition: NodeDefinition;
    direction?: 'in' | 'out';
    extra?: Array<{ nodeId: string; definition: NodeDefinition }>;
  };
}): ManualProposalInput {
  const id = uniqueNodeId(
    input.existingNodeIds,
    `${slugFromType(input.definition.type)}_${input.suffix ?? randomSuffix()}`,
  );
  const inbound = input.connectFrom?.direction === 'in';
  const sources = input.connectFrom
    ? [
        { nodeId: input.connectFrom.nodeId, definition: input.connectFrom.definition },
        ...(input.connectFrom.extra ?? []),
      ]
    : [];
  const matches = sources.map((source) => ({
    source,
    match: inbound
      ? matchingConnectionPorts(input.definition, source.definition)
      : matchingConnectionPorts(source.definition, input.definition),
  }));
  const primary = matches[0];
  if (input.connectFrom && !primary?.match) {
    throw new Error('连线失败：这两张卡没有可接的端口');
  }
  const ops: ManualProposalInput['ops'] = [{
    op: 'spawn_node',
    id,
    node_type: input.definition.type,
    title: input.definition.title,
    params: defaultParamsForDefinition(input.definition),
    pos: [input.position.x, input.position.y],
    ...(inbound || !primary?.match ? {} : { from: primary.source.nodeId }),
  }];
  if (isMediaNodeType(input.definition.type) || input.definition.type === 'input.text') {
    ops.push({
      op: 'resize_node',
      id,
      size: [MEDIA_CARD_WIDTH, MEDIA_CARD_HEIGHT],
    });
  }
  for (const item of matches) {
    if (!item.match) continue;
    if (!inbound && item === primary) continue;
    ops.push({
      op: 'add_edge',
      from: inbound
        ? [id, item.match.sourcePort]
        : [item.source.nodeId, item.match.sourcePort],
      to: inbound
        ? [item.source.nodeId, item.match.targetPort]
        : [id, item.match.targetPort],
      edge_type: item.match.type.toLowerCase(),
    });
  }
  return {
    baseVersionId: input.baseVersionId,
    label: `添加节点 ${id}`,
    ops,
  };
}

export function buildDuplicateNodeProposalInput(input: {
  baseVersionId: string;
  existingNodeIds: Iterable<string>;
  source: GraphNodeState;
  params: unknown;
}): ManualProposalInput {
  const id = uniqueNodeId(input.existingNodeIds, slugFromType(input.source.nodeType));
  const ops: ManualProposalInput['ops'] = [{
    op: 'spawn_node',
    id,
    node_type: input.source.nodeType,
    title: `${input.source.title} copy`,
    params: input.params && typeof input.params === 'object' ? input.params : {},
    pos: [input.source.position.x + 30, input.source.position.y + 30],
    from: input.source.id,
  }];
  const size = input.source.size;
  if (size) {
    ops.push({ op: 'resize_node', id, size: [size.width, size.height] });
  } else if (isMediaNodeType(input.source.nodeType)) {
    ops.push({ op: 'resize_node', id, size: [MEDIA_CARD_WIDTH, MEDIA_CARD_HEIGHT] });
  }
  return {
    baseVersionId: input.baseVersionId,
    label: `复制 ${input.source.title}`,
    ops,
  };
}

export function buildGridSplitProposalInput(input: {
  baseVersionId: string;
  existingNodeIds: Iterable<string>;
  placements: GridSplitTilePlacement[];
  sourceNodeId: string;
  sourceTitle: string;
  tiles: Array<{
    storageUri: string;
    row: number;
    column: number;
    size?: { width: number; height: number };
  }>;
}): ManualProposalInput {
  const usedIds = new Set(input.existingNodeIds);
  const ops: ManualProposalInput['ops'] = [];
  for (const tile of input.tiles) {
    const placement = input.placements.find(
      (item) => item.row === tile.row && item.column === tile.column,
    );
    if (!placement) {
      throw new Error(`宫格切片 r${tile.row + 1}c${tile.column + 1} 缺少摆放位置`);
    }
    appendDerivedMediaCard(ops, usedIds, {
      sourceNodeId: input.sourceNodeId,
      idBase: `image_grid_r${tile.row + 1}c${tile.column + 1}`,
      nodeType: 'input.image',
      title: `${input.sourceTitle} r${tile.row + 1}c${tile.column + 1}`,
      params: { storage_uri: tile.storageUri },
      pos: [placement.x, placement.y],
      size: tile.size ? [tile.size.width, tile.size.height] : undefined,
    });
  }
  return {
    baseVersionId: input.baseVersionId,
    label: `宫格切分 ${input.tiles.length} 张`,
    ops,
  };
}

export function buildMediaIngestProposalInput(input: {
  baseVersionId: string;
  existingNodeIds: Iterable<string>;
  items: Array<{
    nodeType: 'input.image' | 'input.video' | 'input.audio';
    title: string;
    storageUri: string;
    position: Point;
    size?: { width: number; height: number };
  }>;
}): ManualProposalInput {
  const usedIds = new Set(input.existingNodeIds);
  const ops: ManualProposalInput['ops'] = [];
  for (const item of input.items) {
    const id = uniqueNodeId(usedIds, slugFromType(item.nodeType));
    usedIds.add(id);
    ops.push({
      op: 'spawn_node',
      id,
      node_type: item.nodeType,
      title: item.title,
      params: { storage_uri: item.storageUri },
      pos: [item.position.x, item.position.y],
    });
    if (item.size) {
      ops.push({
        op: 'resize_node',
        id,
        size: [item.size.width, item.size.height],
      });
    }
  }
  return {
    baseVersionId: input.baseVersionId,
    label: `导入 ${input.items.length} 个素材`,
    ops,
  };
}

export function buildCropResultProposalInput(input: {
  baseVersionId: string;
  existingNodeIds: Iterable<string>;
  sourceNodeId: string;
  sourceTitle: string;
  sourceX: number;
  sourceY: number;
  sourceWidth: number;
  storageUri: string;
  size: { width: number; height: number };
}): ManualProposalInput {
  const usedIds = new Set(input.existingNodeIds);
  const ops: ManualProposalInput['ops'] = [];
  appendDerivedMediaCard(ops, usedIds, {
    sourceNodeId: input.sourceNodeId,
    idBase: 'image_crop',
    nodeType: 'input.image',
    title: `${input.sourceTitle} 裁剪`,
    params: { storage_uri: input.storageUri },
    pos: [input.sourceX + input.sourceWidth + 24, input.sourceY],
    size: [input.size.width, input.size.height],
  });
  return {
    baseVersionId: input.baseVersionId,
    label: `裁剪 ${input.sourceTitle}`,
    ops,
  };
}

export function mediaGenerateTarget(input: {
  nodeType: string;
  hasImage: boolean;
  mediaKind?: 'image' | 'video';
}): {
  capabilityId: string;
  mediaKind: 'image' | 'video';
  nodeType: string;
  idBase: string;
  connectFrom: boolean;
} | null {
  if (input.nodeType === 'input.audio') return null;
  if (input.nodeType === 'input.text') {
    if (input.mediaKind === 'video') {
      return {
        capabilityId: 'text_to_video',
        mediaKind: 'video',
        nodeType: 'video.text_to_video',
        idBase: 'video_text_to_video',
        connectFrom: true,
      };
    }
    return {
      capabilityId: 'text_to_image',
      mediaKind: 'image',
      nodeType: 'image.generate',
      idBase: 'image_generate',
      connectFrom: true,
    };
  }
  if (!isVisualMediaCardType(input.nodeType)) return null;
  if (input.nodeType === 'input.video' || input.nodeType.startsWith('video.')) {
    return {
      capabilityId: 'text_to_video',
      mediaKind: 'video',
      nodeType: 'video.text_to_video',
      idBase: 'video_text_to_video',
      connectFrom: false,
    };
  }
  if (input.hasImage) {
    return {
      capabilityId: 'image_edit',
      mediaKind: 'image',
      nodeType: 'image.edit',
      idBase: 'image_edit',
      connectFrom: true,
    };
  }
  return {
    capabilityId: 'text_to_image',
    mediaKind: 'image',
    nodeType: 'image.generate',
    idBase: 'image_generate',
    connectFrom: true,
  };
}

export function buildMediaGenerateProposalInput(input: {
  baseVersionId: string;
  existingNodeIds: Iterable<string>;
  sourceNodeId: string;
  sourceTitle: string;
  sourceX: number;
  sourceY: number;
  sourceWidth: number;
  prompt: string;
  aspectRatio: string;
  hasImage: boolean;
  sourceNodeType?: string;
  semantics?: unknown;
  durationSec?: number;
  count?: number;
  mediaKind?: 'image' | 'video';
}): ManualProposalInput {
  const usedIds = new Set(input.existingNodeIds);
  const ops: ManualProposalInput['ops'] = [];
  const x = input.sourceX + input.sourceWidth + 48;
  const prompt = input.prompt.trim();
  const count = Math.min(4, Math.max(1, Math.round(input.count ?? 1)));
  const durationSec = Math.min(10, Math.max(1, Math.round(input.durationSec ?? 5)));
  const target = mediaGenerateTarget({
    nodeType: input.sourceNodeType ?? (input.hasImage ? 'image.edit' : 'image.generate'),
    hasImage: input.hasImage,
    mediaKind: input.mediaKind,
  }) ?? {
    capabilityId: 'text_to_image',
    mediaKind: 'image' as const,
    nodeType: 'image.generate',
    idBase: 'image_generate',
    connectFrom: true,
  };
  const params = target.mediaKind === 'video'
    ? { prompt, aspect_ratio: input.aspectRatio, duration_sec: durationSec }
    : target.nodeType === 'image.edit'
      ? { prompt }
      : { prompt, aspect_ratio: input.aspectRatio };
  for (let index = 0; index < count; index += 1) {
    appendDerivedMediaCard(ops, usedIds, {
      sourceNodeId: input.sourceNodeId,
      idBase: target.idBase,
      nodeType: target.nodeType,
      title: target.nodeType === 'image.edit' ? `${input.sourceTitle} 生成` : input.sourceTitle,
      params,
      pos: [x, input.sourceY + index * 24],
      size: [MEDIA_CARD_WIDTH, MEDIA_CARD_HEIGHT],
      semantics: input.semantics,
      connectFrom: target.connectFrom,
    });
  }
  return {
    baseVersionId: input.baseVersionId,
    label: `生成 ${input.sourceTitle}`,
    ops,
  };
}

export function buildVideoFrameProposalInput(input: {
  baseVersionId: string;
  existingNodeIds: Iterable<string>;
  sourceNodeId: string;
  sourceTitle: string;
  sourceX: number;
  sourceY: number;
  sourceWidth: number;
  storageUri: string;
  size: { width: number; height: number };
  kind: 'current' | 'first' | 'last';
}): ManualProposalInput {
  const usedIds = new Set(input.existingNodeIds);
  const ops: ManualProposalInput['ops'] = [];
  const label = videoFrameLabel(input.kind);
  appendDerivedMediaCard(ops, usedIds, {
    sourceNodeId: input.sourceNodeId,
    idBase: `image_frame_${input.kind}`,
    nodeType: 'input.image',
    title: `${input.sourceTitle} ${label}`,
    params: { storage_uri: input.storageUri },
    pos: [input.sourceX + input.sourceWidth + 24, input.sourceY],
    size: [input.size.width, input.size.height],
    connectFrom: false,
  });
  return {
    baseVersionId: input.baseVersionId,
    label: `${label} ${input.sourceTitle}`,
    ops,
  };
}

export function videoFrameLabel(kind: 'current' | 'first' | 'last'): string {
  if (kind === 'first') return '首帧';
  if (kind === 'last') return '末帧';
  return '当前帧';
}

export function buildImageCanvasToolResultProposalInput(input: {
  baseVersionId: string;
  existingNodeIds: Iterable<string>;
  sourceNodeId: string;
  sourceTitle: string;
  sourceX: number;
  sourceY: number;
  sourceWidth: number;
  kind: ImageCanvasToolKind;
  storageUri: string;
  size: { width: number; height: number };
}): ManualProposalInput {
  const usedIds = new Set(input.existingNodeIds);
  const ops: ManualProposalInput['ops'] = [];
  const label = imageCanvasToolLabel(input.kind);
  appendDerivedMediaCard(ops, usedIds, {
    sourceNodeId: input.sourceNodeId,
    idBase: `image_${input.kind}`,
    nodeType: 'input.image',
    title: `${input.sourceTitle} ${label}`,
    params: { storage_uri: input.storageUri },
    pos: [input.sourceX + input.sourceWidth + 24, input.sourceY],
    size: [input.size.width, input.size.height],
  });
  return {
    baseVersionId: input.baseVersionId,
    label: `${label} ${input.sourceTitle}`,
    ops,
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
    op: 'spawn_node' as const,
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

function appendDerivedMediaCard(
  ops: ManualProposalInput['ops'],
  usedIds: Set<string>,
  input: {
    sourceNodeId: string;
    idBase: string;
    nodeType: string;
    title: string;
    params: unknown;
    pos: [number, number];
    size?: [number, number];
    semantics?: unknown;
    connectFrom?: boolean;
  },
): string {
  const id = uniqueNodeId(usedIds, input.idBase);
  usedIds.add(id);
  ops.push({
    op: 'spawn_node',
    id,
    node_type: input.nodeType,
    title: input.title,
    params: input.params,
    pos: input.pos,
    ...(input.connectFrom === false ? {} : { from: input.sourceNodeId }),
  });
  if (input.size) {
    ops.push({ op: 'resize_node', id, size: input.size });
  }
  if (input.semantics) {
    ops.push({ op: 'set_semantics', id, semantics: input.semantics });
  }
  return id;
}

export function fanInConnectFrom(
  selectedIds: Iterable<string>,
  nodes: Array<{ id: string; nodeType: string }>,
  definitionByType: Map<string, NodeDefinition>,
  primaryId?: string,
): {
  nodeId: string;
  definition: NodeDefinition;
  extra: Array<{ nodeId: string; definition: NodeDefinition }>;
} | undefined {
  const selected = new Set(selectedIds);
  const orderedIds = primaryId
    ? [primaryId, ...[...selected].filter((id) => id !== primaryId)]
    : [...selected];
  const items = orderedIds.flatMap((id) => {
    const node = nodes.find((item) => item.id === id);
    const definition = node ? definitionByType.get(node.nodeType) : undefined;
    return node && definition ? [{ nodeId: id, definition }] : [];
  });
  const primary = items[0];
  if (!primary) return undefined;
  if (!primaryId && items.length < 2) return undefined;
  return {
    nodeId: primary.nodeId,
    definition: primary.definition,
    extra: items.slice(1),
  };
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
