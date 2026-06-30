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
      provider: z.string().nullable(),
      summary: z.string(),
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

const ProviderHealthSchema = z.object({
  ok: z.boolean(),
  message: z.string().nullable(),
});

const ProviderStatusSchema = z.object({
  id: z.string(),
  displayName: z.string(),
  configured: z.boolean(),
  enabled: z.boolean(),
  label: z.string(),
  endpoint: z.string().nullable(),
  health: ProviderHealthSchema,
});

const ProvidersSchema = z
  .object({
    defaultProvider: z.string(),
    providers: z.array(ProviderStatusSchema),
  })
  .default({
    defaultProvider: 'mock',
    providers: [
      {
        id: 'mock',
        displayName: 'Mock',
        configured: true,
        enabled: true,
        label: 'Mock runtime',
        endpoint: null,
        health: { ok: true, message: 'Mock runtime provider configured' },
      },
    ],
  });

const RunStepSchema = z.object({
  nodeId: z.string(),
  title: z.string(),
  state: RunStepStateSchema,
  provider: z.string().nullable(),
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

const OutputSchema = z.object({
  id: z.string(),
  kind: z.string(),
  title: z.string(),
  storageUri: z.string(),
  selected: z.boolean(),
  meta: z.string(),
  mime: z.string().nullable().optional(),
  preview: z
    .object({
      kind: z.enum(['html', 'text']),
      content: z.string(),
    })
    .optional(),
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
export type RunEventEnvelope = z.infer<typeof RunEventEnvelopeSchema>;
export type RunStepState = z.infer<typeof RunStepStateSchema>;
export type RunStatus = z.infer<typeof RunStatusSchema>;
export type GraphNodeState = WorkbenchState['graph']['nodes'][number];
export type LayoutPositionUpdate = { id: string; x: number; y: number };
export type ChatMessageKind = z.infer<typeof ChatMessageKindSchema>;
export type WorkflowGraph = z.infer<typeof WorkflowGraphSchema>;
export type WorkspaceMessageResponse = z.infer<typeof WorkspaceMessageResponseSchema>;
export type RunConfirmationResponse = z.infer<typeof RunConfirmationResponseSchema>;
export type WorkspaceSummary = z.infer<typeof WorkspaceSummarySchema>;
export type NodeCatalog = z.infer<typeof NodeCatalogSchema>;
export type NodeDefinition = z.infer<typeof NodeDefinitionSchema>;
export type ManualProposalInput = {
  baseVersionId: string;
  title?: string;
  summary?: string;
  op:
    | {
        op: 'add_node';
        id: string;
        node_type: string;
        title?: string;
        params: unknown;
        pos: [number, number];
      }
    | { op: 'remove_node'; id: string }
    | { op: 'set_param'; id: string; key: string; value: unknown }
    | { op: 'add_edge'; from: [string, string]; to: [string, string]; edge_type: string }
    | { op: 'remove_edge'; from: [string, string]; to: [string, string]; edge_type: string };
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
