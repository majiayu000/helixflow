import { z } from 'zod';

export const RunStepStateSchema = z.enum([
  'queued',
  'running',
  'succeeded',
  'failed',
  'skipped',
]);

export const RunStatusSchema = z.enum([
  'queued',
  'estimating',
  'waiting_confirmation',
  'running',
  'succeeded',
  'failed',
  'interrupted',
]);

const CostSchema = z.object({
  amount: z.number(),
  currency: z.string(),
});

const PrimaryChatMessageKindSchema = z.enum([
  'text',
  'chat',
  'proposal_pending',
  'proposal_applied',
  'proposal_dismissed',
  'run_requested',
  'run_failed',
  'agent_status',
]);

const AgentLogMessageKindSchema = z.custom<`agent_log:${string}`>(
  (value) => typeof value === 'string' && value.startsWith('agent_log:'),
);

export const ChatMessageKindSchema = z.union([
  PrimaryChatMessageKindSchema,
  AgentLogMessageKindSchema,
]);

export const WorkflowGraphSchema = z.object({
  schema_version: z.number(),
  nodes: z.record(
    z.string(),
    z.object({
      node_type: z.string(),
      title: z.string(),
      params: z.unknown(),
      pos: z.tuple([z.number(), z.number()]),
      size: z.tuple([z.number(), z.number()]).optional(),
    }),
  ),
  edges: z.array(
    z.object({
      from: z.tuple([z.string(), z.string()]),
      to: z.tuple([z.string(), z.string()]),
      edge_type: z.string(),
    }),
  ),
});

const ChatMessageSchema = z.object({
  id: z.string(),
  role: z.enum(['user', 'agent', 'system']),
  kind: ChatMessageKindSchema.optional(),
  text: z.string(),
  time: z.string(),
  label: z.string().nullish(),
  raw: z.string().nullish(),
  turnMode: z
    .enum(['chat', 'create_workflow', 'modify_workflow', 'debug_workflow', 'run_request'])
    .optional(),
});

const CanvasSizeSchema = z.object({
  width: z.number(),
  height: z.number(),
});

const CanvasActorSchema = z.object({
  actorId: z.string(),
  displayName: z.string(),
});

const CanvasCommentTargetSchema = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('node'),
    nodeId: z.string(),
  }),
  z.object({
    kind: z.literal('edge'),
    edgeId: z.string(),
  }),
  z.object({
    kind: z.literal('position'),
    x: z.number(),
    y: z.number(),
  }),
]);

const CanvasCommentStatusSchema = z.enum(['open', 'resolved']);

const CanvasCommentSchema = z.object({
  id: z.string(),
  target: CanvasCommentTargetSchema,
  body: z.string(),
  author: CanvasActorSchema,
  status: CanvasCommentStatusSchema,
  createdAt: z.string(),
  updatedAt: z.string(),
});

export const CanvasPresenceSchema = z.object({
  actor: CanvasActorSchema,
  cursor: z
    .object({
      x: z.number(),
      y: z.number(),
    })
    .nullable()
    .optional(),
  selection: z
    .object({
      nodeIds: z.array(z.string()).default([]),
      edgeIds: z.array(z.string()).default([]),
    })
    .nullable()
    .optional(),
  viewport: z
    .object({
      x: z.number(),
      y: z.number(),
      zoom: z.number(),
    })
    .nullable()
    .optional(),
});

const GraphStateSchema = z.object({
  nodes: z.array(
    z.object({
      id: z.string(),
      nodeType: z.string(),
      title: z.string(),
      category: z.string(),
      status: RunStepStateSchema,
      position: z.object({
        x: z.number(),
        y: z.number(),
      }),
      size: CanvasSizeSchema.optional(),
      provider: z.string().nullable(),
      summary: z.string(),
      cached: z.boolean().optional(),
    }),
  ),
  edges: z.array(
    z.object({
      id: z.string(),
      from: z.object({
        nodeId: z.string(),
        port: z.string(),
      }),
      to: z.object({
        nodeId: z.string(),
        port: z.string(),
      }),
      kind: z.string(),
    }),
  ),
});

const CanvasPositionSchema = z.object({
  x: z.number(),
  y: z.number(),
});

const CanvasNodeSchema = z.object({
  id: z.string(),
  nodeType: z.string(),
  title: z.string(),
  position: CanvasPositionSchema,
  size: CanvasSizeSchema.nullish(),
  params: z.unknown(),
  runtime: z.unknown().nullable().optional(),
  metadata: z.record(z.string(), z.unknown()).optional().default({}),
});

const CanvasEdgeSchema = z.object({
  id: z.string(),
  from: z.object({
    nodeId: z.string(),
    port: z.string(),
  }),
  to: z.object({
    nodeId: z.string(),
    port: z.string(),
  }),
  kind: z.string(),
});

export const CanvasDocumentSchema = z.object({
  schemaVersion: z.number(),
  workspaceId: z.string(),
  versionId: z.string(),
  seq: z.number(),
  nodes: z.array(CanvasNodeSchema),
  edges: z.array(CanvasEdgeSchema),
  comments: z.array(CanvasCommentSchema).default([]),
  runtime: z.record(z.string(), z.unknown()).default({}),
  metadata: z.record(z.string(), z.unknown()).default({}),
});

const RuntimeProviderStatusSchema = z.object({
  id: z.string(),
  label: z.string(),
  kind: z.string(),
  enabled: z.boolean(),
  status: z.string(),
  message: z.string().nullable().optional(),
  capabilities: z.array(z.string()),
});

const WorkflowBackendStatusSchema = z.object({
  id: z.string(),
  label: z.string(),
  status: z.string(),
});

const ApiConnectorStatusSchema = z.object({
  id: z.string(),
  provider: z.string(),
  capability: z.string(),
  status: z.string(),
});

const ProvidersSchema = z
  .object({
    defaultProvider: z.string(),
    selectedProvider: z.string().optional(),
    runtimeProviders: z.array(RuntimeProviderStatusSchema),
    workflowBackends: z.array(WorkflowBackendStatusSchema),
    apiConnectors: z.array(ApiConnectorStatusSchema),
  })
  .default({
    defaultProvider: 'missing',
    selectedProvider: 'missing',
    runtimeProviders: [
      {
        id: 'missing',
        label: 'Provider state missing',
        kind: 'missing',
        enabled: false,
        status: 'missing',
        message: 'Backend did not include provider state',
        capabilities: [],
      },
    ],
    workflowBackends: [],
    apiConnectors: [],
  })
  .transform((providers) => ({
    ...providers,
    selectedProvider: providers.selectedProvider ?? providers.defaultProvider,
  }));

const RunStepSchema = z.object({
  nodeId: z.string(),
  title: z.string(),
  state: RunStepStateSchema,
  provider: z.string().nullable(),
  cached: z.boolean().optional(),
  error: z
    .object({
      summary: z.string(),
      raw: z.string().nullable().optional(),
    })
    .nullable()
    .optional(),
});

const RunSchema = z.object({
  id: z.string(),
  label: z.string(),
  status: RunStatusSchema,
  error: z
    .object({
      summary: z.string(),
      raw: z.string().nullable().optional(),
    })
    .nullable()
    .optional(),
  steps: z.array(RunStepSchema),
  cost: z.object({
    estimate: z.number(),
    actual: z.number(),
    currency: z.string(),
  }),
});

const OutputPreviewSchema = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('html'),
    content: z.string(),
  }),
  z.object({
    kind: z.literal('text'),
    content: z.string(),
  }),
  z.object({
    kind: z.literal('image'),
    content: z.string(),
    mime: z.string().nullable().optional(),
  }),
  z.object({
    kind: z.literal('video'),
    content: z.string(),
    mime: z.string().nullable().optional(),
  }),
]);

const OutputSchema = z.object({
  id: z.string(),
  kind: z.string(),
  title: z.string(),
  storageUri: z.string(),
  selected: z.boolean(),
  meta: z.string(),
  mime: z.string().nullable().optional(),
  preview: OutputPreviewSchema.optional(),
});

const PendingConfirmationSchema = z.object({
  id: z.string(),
  title: z.string(),
  summary: z.string(),
  cost: CostSchema,
  runCount: z.number().int().positive().optional(),
  pendingChanges: z.array(z.string()).optional(),
  interruptible: z.boolean().optional(),
});

const ProposalSchema = z.object({
  id: z.string(),
  baseVersionId: z.string(),
  kind: z.enum(['create', 'modify', 'fix', 'sweep']),
  title: z.string(),
  summary: z.string(),
  ops: z.array(z.unknown()),
  diffSummary: z.array(z.string()),
  previewGraph: z.union([GraphStateSchema, WorkflowGraphSchema.transform(workflowGraphToGraphState)]),
  state: z.enum(['pending', 'applied', 'dismissed', 'superseded', 'invalid']),
  messageId: z.string().nullable(),
});

const PortDefinitionSchema = z.object({
  name: z.string(),
  type: z.enum(['TEXT', 'IMAGE', 'VIDEO', 'AUDIO', 'MASK', 'JSON']),
  required: z.boolean(),
});

const ParamSpecSchema = z.object({
  type: z.enum(['string', 'integer', 'number', 'boolean']),
  enum_values: z.array(z.unknown()),
  minimum: z.number().nullable(),
  maximum: z.number().nullable(),
});

const ParamsSchema = z.object({
  required: z.array(z.string()),
  properties: z.record(z.string(), ParamSpecSchema),
  allow_unknown: z.boolean(),
});

export const NodeDefinitionSchema = z.object({
  type: z.string(),
  title: z.string(),
  category: z.string(),
  provider: z.string().nullable(),
  capability: z.string().nullable(),
  description: z.string(),
  inputs: z.array(PortDefinitionSchema),
  outputs: z.array(PortDefinitionSchema),
  params_schema: ParamsSchema,
  estimated_cost: z.unknown().nullable(),
});

export const NodeCatalogSchema = z.object({
  schema_version: z.number(),
  nodes: z.array(NodeDefinitionSchema),
});

export const CanvasSelectionContextSchema = z
  .object({
    nodeIds: z.array(z.string()),
  })
  .strict();

export const CanvasMessageContextSchema = z
  .object({
    selection: CanvasSelectionContextSchema,
  })
  .strict();

export const WorkbenchStateSchema = z.object({
  eventSeq: z.number(),
  workspace: z.object({
    id: z.string(),
    name: z.string(),
    versionId: z.string(),
    updatedAt: z.string(),
  }),
  providers: ProvidersSchema,
  chat: z.object({
    messages: z.array(ChatMessageSchema),
  }),
  graph: GraphStateSchema,
  run: RunSchema.nullable(),
  outputs: z.array(OutputSchema),
  history: z.array(
    z.object({
      id: z.string(),
      kind: z.enum(['version', 'run', 'proposal']),
      label: z.string(),
      time: z.string(),
      summary: z.string(),
    }),
  ),
  pendingConfirmation: PendingConfirmationSchema.nullable(),
  pendingProposal: ProposalSchema.nullable(),
  workflowGraph: WorkflowGraphSchema.optional(),
});

export const WorkspaceMessageResponseSchema = z.object({
  turnMode: z.enum(['chat', 'create_workflow', 'modify_workflow', 'debug_workflow', 'run_request']),
  messages: z.array(ChatMessageSchema),
  proposal: ProposalSchema.nullable(),
  run: RunSchema.nullable().optional(),
  pendingConfirmation: PendingConfirmationSchema.nullable().optional(),
});

export const RunConfirmationResponseSchema = z.object({
  run: RunSchema,
  outputs: z.array(OutputSchema),
  pendingConfirmation: PendingConfirmationSchema.nullable(),
});

export const WorkspaceSummarySchema = z.object({
  id: z.string(),
  name: z.string(),
  versionId: z.string(),
  createdAt: z.string().optional().default(''),
  updatedAt: z.string(),
  firstMessage: z.string().nullable().optional().default(null),
  messageCount: z.number().optional().default(0),
});

export const RunEventEnvelopeSchema = z.object({
  workspace_id: z.string(),
  run_id: z.string(),
  seq: z.number(),
  server_time: z.string(),
  ev: z.string(),
  data: z.record(z.string(), z.unknown()),
});

export type WorkbenchState = z.infer<typeof WorkbenchStateSchema>;
export type CanvasDocument = z.infer<typeof CanvasDocumentSchema>;
export type CanvasActor = z.infer<typeof CanvasActorSchema>;
export type CanvasComment = z.infer<typeof CanvasCommentSchema>;
export type CanvasCommentTarget = z.infer<typeof CanvasCommentTargetSchema>;
export type CanvasPresence = z.infer<typeof CanvasPresenceSchema>;
export type RunEventEnvelope = z.infer<typeof RunEventEnvelopeSchema>;
export type RunStepState = z.infer<typeof RunStepStateSchema>;
export type RunStatus = z.infer<typeof RunStatusSchema>;
export type GraphNodeState = WorkbenchState['graph']['nodes'][number];
export type LayoutPositionUpdate = { id: string; x: number; y: number };
export type NodeSizeUpdate = { id: string; width: number; height: number };
export type ChatMessageKind = z.infer<typeof ChatMessageKindSchema>;
export type WorkflowGraph = z.infer<typeof WorkflowGraphSchema>;
export type WorkspaceMessageResponse = z.infer<typeof WorkspaceMessageResponseSchema>;
export type RunConfirmationResponse = z.infer<typeof RunConfirmationResponseSchema>;
export type WorkspaceSummary = z.infer<typeof WorkspaceSummarySchema>;
export type NodeCatalog = z.infer<typeof NodeCatalogSchema>;
export type NodeDefinition = z.infer<typeof NodeDefinitionSchema>;
export type CanvasMessageContext = z.infer<typeof CanvasMessageContextSchema>;
export type GraphState = WorkbenchState['graph'];
export type ManualProposalInput = {
  baseVersionId: string;
  idempotencyKey?: string;
  label?: string;
  ops: Array<
    | {
        op: 'add_node';
        id: string;
        node_type: string;
        title?: string;
        params: unknown;
        pos: [number, number];
      }
    | { op: 'remove_node'; id: string }
    | { op: 'set_param'; id: string; key: string; prev?: unknown; value: unknown }
    | { op: 'add_edge'; from: [string, string]; to: [string, string]; edge_type: string }
    | { op: 'remove_edge'; from: [string, string]; to: [string, string]; edge_type: string }
    | { op: 'move_node'; id: string; pos: [number, number] }
    | { op: 'resize_node'; id: string; size: [number, number] }
  >;
};
export type ManualEditOp = ManualProposalInput['ops'][number];
export type ManualEditSession = {
  baseVersionId: string;
  idempotencyKey: string;
  source: 'user';
  ops: ManualEditOp[];
  startedAt: string;
};
export type CanvasCommentOpInput = {
  baseSeq?: number;
  op:
    | {
        op: 'comment_add';
        id?: string;
        target: CanvasCommentTarget;
        body: string;
        actor?: CanvasActor;
      }
    | { op: 'comment_patch'; id: string; body?: string; status?: CanvasComment['status'] }
    | { op: 'comment_delete'; id: string };
};

function workflowGraphToGraphState(
  graph: z.infer<typeof WorkflowGraphSchema>,
): z.infer<typeof GraphStateSchema> {
  return {
    nodes: Object.entries(graph.nodes).map(([id, node]) => ({
      id,
      nodeType: node.node_type,
      title: node.title,
      category: nodeCategory(node.node_type),
      status: 'queued',
      position: { x: node.pos[0], y: node.pos[1] },
      size: node.size ? { width: node.size[0], height: node.size[1] } : undefined,
      provider: null,
      summary: nodeSummary(node.node_type, node.params),
    })),
    edges: graph.edges.map((edge, index) => ({
      id: `edge_${edge.from[0]}_${edge.from[1]}_${edge.to[0]}_${edge.to[1]}_${index}`,
      from: { nodeId: edge.from[0], port: edge.from[1] },
      to: { nodeId: edge.to[0], port: edge.to[1] },
      kind: edge.edge_type,
    })),
  };
}

export function graphStateFromCanvasDocument(
  canvas: CanvasDocument,
  fallback?: GraphState,
): GraphState {
  const fallbackById = new Map((fallback?.nodes ?? []).map((node) => [node.id, node] as const));
  return {
    nodes: canvas.nodes.map((node) => {
      const fallbackNode = fallbackById.get(node.id);
      return {
        id: node.id,
        nodeType: node.nodeType,
        title: node.title,
        category: fallbackNode?.category ?? nodeCategory(node.nodeType),
        status: fallbackNode?.status ?? 'queued',
        position: node.position,
        size: node.size ?? undefined,
        provider: fallbackNode?.provider ?? null,
        summary: nodeSummary(node.nodeType, node.params),
        cached: fallbackNode?.cached,
      };
    }),
    edges: canvas.edges.map((edge) => ({
      id: edge.id,
      from: edge.from,
      to: edge.to,
      kind: edge.kind,
    })),
  };
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

function nodeSummary(nodeType: string, params: unknown): string {
  if (params && typeof params === 'object' && !Array.isArray(params)) {
    const summary = (params as Record<string, unknown>).summary;
    if (typeof summary === 'string') {
      return summary;
    }
  }
  return nodeType;
}
