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

export const ChatMessageKindSchema = z.enum([
  'text',
  'chat',
  'proposal_pending',
  'proposal_applied',
  'proposal_dismissed',
  'run_requested',
  'run_failed',
  'agent_log:status',
  'agent_log:tool_call',
  'agent_log:tool_result',
  'agent_log:error',
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
});

const RunStepSchema = z.object({
  nodeId: z.string(),
  title: z.string(),
  state: RunStepStateSchema,
  provider: z.string().nullable(),
});

const RunSchema = z.object({
  id: z.string(),
  label: z.string(),
  status: RunStatusSchema,
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
});

const PendingConfirmationSchema = z.object({
  id: z.string(),
  title: z.string(),
  summary: z.string(),
  cost: CostSchema,
});

const ProposalSchema = z.object({
  id: z.string(),
  baseVersionId: z.string(),
  kind: z.enum(['create', 'modify', 'fix', 'sweep']),
  title: z.string(),
  summary: z.string(),
  ops: z.array(z.unknown()),
  diffSummary: z.array(z.string()),
  previewGraph: WorkflowGraphSchema,
  state: z.enum(['pending', 'applied', 'dismissed', 'superseded', 'invalid']),
  messageId: z.string().nullable(),
});

export const WorkbenchStateSchema = z.object({
  eventSeq: z.number(),
  workspace: z.object({
    id: z.string(),
    name: z.string(),
    versionId: z.string(),
    updatedAt: z.string(),
  }),
  chat: z.object({
    messages: z.array(ChatMessageSchema),
  }),
  graph: z.object({
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
  }),
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
  updatedAt: z.string(),
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
export type ChatMessageKind = z.infer<typeof ChatMessageKindSchema>;
export type WorkflowGraph = z.infer<typeof WorkflowGraphSchema>;
export type WorkspaceMessageResponse = z.infer<typeof WorkspaceMessageResponseSchema>;
export type RunConfirmationResponse = z.infer<typeof RunConfirmationResponseSchema>;
export type WorkspaceSummary = z.infer<typeof WorkspaceSummarySchema>;
