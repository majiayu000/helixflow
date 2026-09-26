import { lineageConnection } from './components/graph-canvas-connections';
import type {
  CanvasSnapshotUpdate,
  ManualEditOp,
  ManualEditSession,
  ManualProposalInput,
  WorkbenchState,
  WorkflowGraph,
} from './types';

export type QueueLockReason =
  | { kind: 'none' }
  | { kind: 'busy' }
  | { kind: 'active_run' }
  | { kind: 'empty_graph' }
  | { kind: 'provider_unavailable'; message: string }
  | { kind: 'pending_confirmation' }
  | { kind: 'pending_proposal' }
  | { kind: 'dirty_edits'; count: number };

export function hasDirtyEdits(session: ManualEditSession | null): session is ManualEditSession {
  return Boolean(session && session.ops.length > 0);
}

/** Layout lives on the canvas snapshot API; graph structure still uses versions/ops. */
export function partitionManualEditOps(ops: ManualEditOp[]): {
  graphOps: ManualEditOp[];
  snapshot: CanvasSnapshotUpdate | null;
} {
  const graphOps: ManualEditOp[] = [];
  const positions = new Map<string, { id: string; x: number; y: number }>();
  const sizes = new Map<string, { id: string; width: number; height: number }>();
  for (const op of ops) {
    if (op.op === 'move_node') {
      positions.set(op.id, { id: op.id, x: op.pos[0], y: op.pos[1] });
      continue;
    }
    if (op.op === 'resize_node') {
      sizes.set(op.id, { id: op.id, width: op.size[0], height: op.size[1] });
      continue;
    }
    graphOps.push(op);
  }
  if (positions.size === 0 && sizes.size === 0) {
    return { graphOps, snapshot: null };
  }
  return {
    graphOps,
    snapshot: {
      ...(positions.size > 0 ? { positions: [...positions.values()] } : {}),
      ...(sizes.size > 0 ? { sizes: [...sizes.values()] } : {}),
    },
  };
}

export function layoutOpsFromSnapshot(snapshot: CanvasSnapshotUpdate): ManualEditOp[] {
  return [
    ...(snapshot.positions ?? []).map((position) => ({
      op: 'move_node' as const,
      id: position.id,
      pos: [position.x, position.y] as [number, number],
    })),
    ...(snapshot.sizes ?? []).map((size) => ({
      op: 'resize_node' as const,
      id: size.id,
      size: [size.width, size.height] as [number, number],
    })),
  ];
}

export function appendManualEditInput(
  session: ManualEditSession | null,
  baseVersionId: string,
  input: ManualProposalInput,
  now = new Date().toISOString(),
): ManualEditSession {
  if (input.ops.length === 0) {
    throw new Error('manual edit input must contain at least one op');
  }
  const ops = input.baseVersionId === baseVersionId
    ? input.ops
    : input.ops.map((op) => {
        if (op.op !== 'set_param') return op;
        const { prev: _stalePrev, ...rebased } = op;
        return rebased;
      });

  const current =
    session?.baseVersionId === baseVersionId
      ? session
      : {
          baseVersionId,
          idempotencyKey: input.idempotencyKey ?? createCanvasOpIdempotencyKey(),
          source: 'user' as const,
          ops: [],
          startedAt: now,
        };

  return {
    ...current,
    ops: [...current.ops, ...ops],
  };
}

export function manualEditInputFromSession(session: ManualEditSession): ManualProposalInput {
  return {
    baseVersionId: session.baseVersionId,
    idempotencyKey: session.idempotencyKey,
    label: `Manual edit session (${session.ops.length} changes)`,
    ops: session.ops,
  };
}

export function manualProposalWithIdempotency(input: ManualProposalInput): ManualProposalInput {
  if (input.idempotencyKey && input.idempotencyKey.trim()) {
    return input;
  }
  return { ...input, idempotencyKey: createCanvasOpIdempotencyKey() };
}

export function createCanvasOpIdempotencyKey(): string {
  const randomUuid = globalThis.crypto?.randomUUID?.();
  if (randomUuid) {
    return `canvas_op_${randomUuid}`;
  }
  const suffix = Math.random().toString(36).slice(2, 12);
  return `canvas_op_${Date.now()}_${suffix}`;
}

export function manualEditSummary(session: ManualEditSession | null, limit = 6): string[] {
  if (!session) return [];
  const rows = session.ops.map(manualEditOpSummary);
  if (rows.length <= limit) return rows;
  return [...rows.slice(0, limit), `+${rows.length - limit} more changes`];
}

export function buildSetParamEditInput(
  baseVersionId: string,
  workflowGraph: WorkflowGraph | undefined,
  nodeId: string,
  key: string,
  value: unknown,
): ManualProposalInput {
  const prev = workflowParamPrev(workflowGraph, nodeId, key);
  return {
    baseVersionId,
    label: `Inspector set ${key}`,
    ops: [
      prev.found
        ? { op: 'set_param', id: nodeId, key, prev: prev.value, value }
        : { op: 'set_param', id: nodeId, key, value },
    ],
  };
}

export function graphStateWithWorkflowLayout(
  graph: WorkbenchState['graph'],
  workflowGraph: WorkflowGraph | undefined,
): WorkbenchState['graph'] {
  if (!workflowGraph) return graph;
  return {
    ...graph,
    nodes: graph.nodes.map((node) => {
      const workflowNode = workflowGraph.nodes[node.id];
      if (!workflowNode) return node;
      const size = workflowNode.size
        ? { width: workflowNode.size[0], height: workflowNode.size[1] }
        : node.size ?? undefined;
      return {
        ...node,
        position: { x: workflowNode.pos[0], y: workflowNode.pos[1] },
        size,
      };
    }),
  };
}

export function previewWorkbenchStateWithManualEdits(
  state: WorkbenchState,
  session: ManualEditSession | null,
): WorkbenchState {
  if (!session || session.baseVersionId !== state.workspace.versionId || session.ops.length === 0) {
    return state;
  }
  const preview = previewManualEditGraph(state.graph, state.workflowGraph, session.ops);
  return {
    ...state,
    graph: graphStateWithWorkflowLayout(preview.graph, preview.workflowGraph),
    workflowGraph: preview.workflowGraph,
  };
}

export function deriveQueueLockReason(input: {
  busy: boolean;
  activeRun: boolean;
  graphNodeCount: number;
  hasProviderNodes: boolean;
  providerReady: boolean;
  providerMessage: string;
  pendingConfirmation: boolean;
  pendingProposal: boolean;
  dirtyEditCount: number;
}): QueueLockReason {
  if (input.activeRun) return { kind: 'active_run' };
  if (input.busy) return { kind: 'busy' };
  if (input.pendingProposal) return { kind: 'pending_proposal' };
  if (input.dirtyEditCount > 0) {
    return { kind: 'dirty_edits', count: input.dirtyEditCount };
  }
  if (input.pendingConfirmation) return { kind: 'pending_confirmation' };
  if (input.graphNodeCount === 0) return { kind: 'empty_graph' };
  if (input.hasProviderNodes && !input.providerReady) {
    return { kind: 'provider_unavailable', message: input.providerMessage };
  }
  return { kind: 'none' };
}

export function queueLockReasonTitle(reason: QueueLockReason): string {
  switch (reason.kind) {
    case 'none':
      return '提交当前工作流运行';
    case 'busy':
      return '等待当前请求完成';
    case 'active_run':
      return '当前运行中，可中断';
    case 'empty_graph':
      return '空工作流不能运行';
    case 'provider_unavailable':
      return reason.message;
    case 'pending_confirmation':
      return '先确认或取消当前 run cost gate';
    case 'pending_proposal':
      return '先应用或忽略待审核 proposal';
    case 'dirty_edits':
      return `先提交或放弃 ${reason.count} 个手动编辑`;
  }
}

export function queueLockReasonLabel(reason: QueueLockReason): string {
  switch (reason.kind) {
    case 'none':
      return 'Queue ready';
    case 'busy':
      return '请求处理中';
    case 'active_run':
      return '运行中';
    case 'empty_graph':
      return '空工作流';
    case 'provider_unavailable':
      return 'Provider 不可用';
    case 'pending_confirmation':
      return '等待 cost gate';
    case 'pending_proposal':
      return '待审 proposal';
    case 'dirty_edits':
      return `EDITING · ${reason.count} CHANGES`;
  }
}

function previewManualEditGraph(
  graph: WorkbenchState['graph'],
  workflowGraph: WorkflowGraph | undefined,
  ops: ManualEditOp[],
): { graph: WorkbenchState['graph']; workflowGraph?: WorkflowGraph } {
  let nodes = graph.nodes.map((node) => ({
    ...node,
    position: { ...node.position },
  }));
  let edges = graph.edges.map((edge) => ({
    ...edge,
    from: { ...edge.from },
    to: { ...edge.to },
  }));
  const workflow = workflowGraph ? cloneWorkflowGraph(workflowGraph) : undefined;

  for (const op of ops) {
    if (op.op === 'move_node') {
      nodes = nodes.map((node) =>
        node.id === op.id ? { ...node, position: { x: op.pos[0], y: op.pos[1] } } : node,
      );
      if (workflow?.nodes[op.id]) {
        workflow.nodes[op.id] = { ...workflow.nodes[op.id], pos: op.pos };
      }
    }
    if (op.op === 'resize_node') {
      nodes = nodes.map((node) =>
        node.id === op.id ? { ...node, size: { width: op.size[0], height: op.size[1] } } : node,
      );
      if (workflow?.nodes[op.id]) {
        workflow.nodes[op.id] = { ...workflow.nodes[op.id], size: op.size };
      }
    }
    if (op.op === 'set_semantics') {
      if (workflow?.nodes[op.id]) {
        workflow.nodes[op.id] = {
          ...workflow.nodes[op.id],
          semantics: cloneUnknown(op.semantics),
        };
      }
    }
    if (op.op === 'set_param') {
      if (workflow?.nodes[op.id]) {
        const params = objectParams(workflow.nodes[op.id].params);
        workflow.nodes[op.id] = {
          ...workflow.nodes[op.id],
          params: { ...params, [op.key]: cloneUnknown(op.value) },
        };
      }
      nodes = nodes.map((node) =>
        node.id === op.id ? { ...node, summary: `${op.key}: ${shortValue(op.value)}` } : node,
      );
    }
    if (op.op === 'spawn_node') {
      const title = op.title?.trim() || op.id;
      nodes = [
        ...nodes.filter((node) => node.id !== op.id),
        {
          id: op.id,
          nodeType: op.node_type,
          title,
          category: nodeCategory(op.node_type),
          status: 'queued',
          position: { x: op.pos[0], y: op.pos[1] },
          provider: null,
          summary: op.node_type,
        },
      ];
      if (workflow) {
        workflow.nodes[op.id] = {
          node_type: op.node_type,
          title,
          params: cloneUnknown(op.params),
          pos: op.pos,
        };
      }
      if (op.from) {
        const source = nodes.find((node) => node.id === op.from);
        const sourceType = source?.nodeType ?? workflow?.nodes[op.from]?.node_type;
        if (!sourceType) {
          throw new Error(`连线失败：源节点 ${op.from} 不存在`);
        }
        const match = lineageConnection(sourceType, op.node_type);
        const from = [op.from, match.sourcePort] as [string, string];
        const to = [op.id, match.targetPort] as [string, string];
        const edgeType = match.type.toLowerCase();
        const signature = edgeSignature(from, to, edgeType);
        if (!edges.some((edge) => edgeSignatureFromGraphEdge(edge) === signature)) {
          edges = [
            ...edges,
            {
              id: `edge_${from[0]}_${from[1]}_${to[0]}_${to[1]}_${edges.length}`,
              from: { nodeId: from[0], port: from[1] },
              to: { nodeId: to[0], port: to[1] },
              kind: edgeType,
            },
          ];
        }
        if (workflow && !workflow.edges.some((edge) => edgeSignature(edge.from, edge.to, edge.edge_type) === signature)) {
          workflow.edges = [
            ...workflow.edges,
            { from, to, edge_type: edgeType },
          ];
        }
      }
    }
    if (op.op === 'remove_node') {
      nodes = nodes.filter((node) => node.id !== op.id);
      edges = edges.filter((edge) => edge.from.nodeId !== op.id && edge.to.nodeId !== op.id);
      if (workflow) {
        delete workflow.nodes[op.id];
        workflow.edges = workflow.edges.filter((edge) => edge.from[0] !== op.id && edge.to[0] !== op.id);
      }
    }
    if (op.op === 'add_edge') {
      const signature = edgeSignature(op.from, op.to, op.edge_type);
      if (!edges.some((edge) => edgeSignatureFromGraphEdge(edge) === signature)) {
        edges = [
          ...edges,
          {
            id: `edge_${op.from[0]}_${op.from[1]}_${op.to[0]}_${op.to[1]}_${edges.length}`,
            from: { nodeId: op.from[0], port: op.from[1] },
            to: { nodeId: op.to[0], port: op.to[1] },
            kind: op.edge_type,
          },
        ];
      }
      if (workflow && !workflow.edges.some((edge) => edgeSignature(edge.from, edge.to, edge.edge_type) === signature)) {
        workflow.edges = [
          ...workflow.edges,
          { from: op.from, to: op.to, edge_type: op.edge_type },
        ];
      }
    }
    if (op.op === 'remove_edge') {
      const signature = edgeSignature(op.from, op.to, op.edge_type);
      edges = edges.filter((edge) => edgeSignatureFromGraphEdge(edge) !== signature);
      if (workflow) {
        workflow.edges = workflow.edges.filter(
          (edge) => edgeSignature(edge.from, edge.to, edge.edge_type) !== signature,
        );
      }
    }
  }

  return { graph: { nodes, edges }, workflowGraph: workflow };
}

function manualEditOpSummary(op: ManualEditOp): string {
  switch (op.op) {
    case 'move_node':
      return `Move ${op.id} to ${Math.round(op.pos[0])}, ${Math.round(op.pos[1])}`;
    case 'resize_node':
      return `Resize ${op.id} to ${Math.round(op.size[0])} x ${Math.round(op.size[1])}`;
    case 'set_semantics':
      return `Pin model on ${op.id}`;
    case 'set_param':
      return `Set ${op.id}.${op.key}`;
    case 'spawn_node':
      return op.from ? `Spawn ${op.id} from ${op.from}` : `Spawn ${op.id}`;
    case 'remove_node':
      return `Remove ${op.id}`;
    case 'add_edge':
      return `Connect ${op.from.join('.')} -> ${op.to.join('.')}`;
    case 'remove_edge':
      return `Disconnect ${op.from.join('.')} -> ${op.to.join('.')}`;
  }
}

function cloneWorkflowGraph(graph: WorkflowGraph): WorkflowGraph {
  return {
    schema_version: graph.schema_version,
    nodes: Object.fromEntries(
      Object.entries(graph.nodes).map(([id, node]) => [
        id,
        {
          node_type: node.node_type,
          title: node.title,
          params: cloneUnknown(node.params),
          pos: [...node.pos] as [number, number],
          size: node.size ? ([...node.size] as [number, number]) : undefined,
          semantics: cloneUnknown(node.semantics),
        },
      ]),
    ),
    edges: graph.edges.map((edge) => ({
      from: [...edge.from] as [string, string],
      to: [...edge.to] as [string, string],
      edge_type: edge.edge_type,
    })),
  };
}

function objectParams(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function workflowParamPrev(
  graph: WorkflowGraph | undefined,
  nodeId: string,
  key: string,
): { found: true; value: unknown } | { found: false } {
  const params = graph?.nodes[nodeId]?.params;
  if (!params || typeof params !== 'object' || Array.isArray(params)) {
    return { found: false };
  }
  if (!Object.prototype.hasOwnProperty.call(params, key)) {
    return { found: false };
  }
  return { found: true, value: (params as Record<string, unknown>)[key] };
}

function cloneUnknown<T>(value: T): T {
  if (value === undefined) return value;
  return JSON.parse(JSON.stringify(value)) as T;
}

function edgeSignature(from: [string, string], to: [string, string], kind: string): string {
  return `${from[0]}:${from[1]}>${to[0]}:${to[1]}:${kind}`;
}

function edgeSignatureFromGraphEdge(edge: WorkbenchState['graph']['edges'][number]): string {
  return `${edge.from.nodeId}:${edge.from.port}>${edge.to.nodeId}:${edge.to.port}:${edge.kind}`;
}

function nodeCategory(nodeType: string): string {
  const prefix = nodeType.split('.')[0] ?? 'node';
  if (prefix === 'input') return 'Input';
  if (prefix === 'output') return 'Output';
  if (prefix === 'llm') return 'Text';
  if (prefix === 'image') return 'Image';
  if (prefix === 'video') return 'Video';
  return 'Node';
}

function shortValue(value: unknown): string {
  if (typeof value === 'string') return value.slice(0, 48);
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return JSON.stringify(value).slice(0, 48);
}
