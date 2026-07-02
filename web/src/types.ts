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

export const WorkbenchStateSchema = z.object({
  eventSeq: z.number(),
  workspace: z.object({
    id: z.string(),
    name: z.string(),
    versionId: z.string(),
    updatedAt: z.string(),
  }),
  providers: z.object({
    defaultProvider: z.string(),
    providers: z.array(ProviderStatusSchema),
  }),
  chat: z.object({
    messages: z.array(
      z.object({
        id: z.string(),
        role: z.enum(['user', 'agent', 'system']),
        text: z.string(),
        time: z.string(),
        kind: z.string().nullish(),
        label: z.string().nullish(),
        raw: z.string().nullish(),
      }),
    ),
  }),
  graph: GraphStateSchema,
  run: z.object({
    id: z.string(),
    label: z.string(),
    status: RunStatusSchema,
    steps: z.array(
      z.object({
        nodeId: z.string(),
        title: z.string(),
        state: RunStepStateSchema,
        provider: z.string().nullable(),
      }),
    ),
    cost: z.object({
      estimate: z.number(),
      actual: z.number(),
      currency: z.string(),
    }),
  }),
  outputs: z.array(
    z.object({
      id: z.string(),
      kind: z.enum(['text', 'image', 'video', 'json', 'html', 'markdown']),
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
    }),
  ),
  history: z.array(
    z.object({
      id: z.string(),
      kind: z.enum(['version', 'run', 'proposal']),
      label: z.string(),
      time: z.string(),
      summary: z.string(),
    }),
  ),
  pendingProposal: z
    .object({
      id: z.string(),
      title: z.string(),
      summary: z.string(),
      diffSummary: z.array(z.string()),
      previewGraph: GraphStateSchema,
    })
    .nullable(),
  pendingConfirmation: z
    .object({
      id: z.string(),
      title: z.string(),
      summary: z.string(),
      cost: CostSchema,
    })
    .nullable(),
});

export const WorkspaceListSchema = z.object({
  workspaces: z.array(
    z.object({
      id: z.string(),
      name: z.string(),
      versionId: z.string().nullable(),
      createdAt: z.string(),
      updatedAt: z.string(),
      firstMessage: z.string().nullable(),
      messageCount: z.number(),
    }),
  ),
});

export const RunEventEnvelopeSchema = z.object({
  workspace_id: z.string(),
  run_id: z.string(),
  seq: z.number(),
  server_time: z.string(),
  ev: z.string(),
  data: z.record(z.string(), z.unknown()),
});

const CanvasPointSchema = z.object({
  x: z.number(),
  y: z.number(),
});

const CanvasSizeSchema = z.object({
  width: z.number(),
  height: z.number(),
});

const CanvasPortSchema = z.object({
  name: z.string(),
  type: z.string(),
  required: z.boolean(),
});

const CanvasPortsSchema = z.object({
  inputs: z.array(CanvasPortSchema),
  outputs: z.array(CanvasPortSchema),
});

export const CanvasActorKindSchema = z.enum(['user', 'agent', 'system', 'job_worker']);
export const CanvasOpKindSchema = z.enum([
  'node_add',
  'node_patch',
  'node_move',
  'node_resize',
  'node_delete',
  'edge_add',
  'edge_delete',
  'comment_add',
  'comment_patch',
  'comment_delete',
  'run_request',
  'artifact_attach',
  'proposal_apply',
]);

export const CanvasActorSchema = z.object({
  id: z.string(),
  kind: CanvasActorKindSchema,
});

export const CanvasNodeSchema = z.object({
  id: z.string(),
  kind: z.enum([
    'workflow',
    'text',
    'image',
    'video',
    'audio',
    'comment',
    'group',
    'artifact',
    'config',
  ]),
  node_type: z.string().nullable().optional(),
  title: z.string(),
  position: CanvasPointSchema,
  size: CanvasSizeSchema.nullable().optional(),
  ports: CanvasPortsSchema,
  params: z.unknown(),
  content: z.unknown(),
  media: z.array(
    z.object({
      id: z.string(),
      kind: z.enum(['upload', 'artifact', 'external']),
      uri: z.string(),
      artifact_id: z.string().nullable().optional(),
      upload_id: z.string().nullable().optional(),
    }),
  ),
  runtime: z.object({
    status: z.enum(['idle', 'queued', 'running', 'succeeded', 'failed', 'skipped']),
    run_id: z.string().nullable().optional(),
    run_step_id: z.string().nullable().optional(),
    artifact_ids: z.array(z.string()),
    error: z.string().nullable().optional(),
  }),
  ui: z.object({
    z_index: z.number(),
    collapsed: z.boolean(),
    locked: z.boolean(),
    color: z.string().nullable().optional(),
  }),
  created_at: z.string(),
  updated_at: z.string(),
});

export const CanvasEdgeSchema = z.object({
  id: z.string(),
  from: z.object({
    node_id: z.string(),
    port: z.string(),
  }),
  to: z.object({
    node_id: z.string(),
    port: z.string(),
  }),
  kind: z.enum(['data', 'artifact', 'control', 'reference', 'visual']),
  edge_type: z.string().nullable().optional(),
  label: z.string().nullable().optional(),
  metadata: z.unknown(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const CanvasCommentSchema = z.object({
  id: z.string(),
  anchor: z.object({
    node_id: z.string().nullable().optional(),
    edge_id: z.string().nullable().optional(),
    position: CanvasPointSchema.nullable().optional(),
  }),
  body: z.string(),
  resolved: z.boolean(),
  author_id: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const CanvasDocumentSchema = z.object({
  schema_version: z.number(),
  canvas_id: z.string(),
  workspace_id: z.string(),
  document_version_id: z.string().nullable().optional(),
  title: z.string(),
  seq: z.number(),
  base_graph_version_id: z.string().nullable().optional(),
  viewport: z.object({
    x: z.number(),
    y: z.number(),
    zoom: z.number(),
  }),
  nodes: z.record(z.string(), CanvasNodeSchema),
  edges: z.record(z.string(), CanvasEdgeSchema),
  comments: z.record(z.string(), CanvasCommentSchema),
  metadata: z.unknown(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const CanvasOpEnvelopeSchema = z.object({
  op_id: z.string(),
  canvas_id: z.string(),
  seq: z.number(),
  base_seq: z.number(),
  actor: CanvasActorSchema,
  kind: CanvasOpKindSchema,
  payload: z.unknown(),
  idempotency_key: z.string(),
  created_at: z.string(),
});

export const CanvasSnapshotResponseSchema = z.object({
  canvas: CanvasDocumentSchema,
});

export const CanvasOpsResponseSchema = z.object({
  canvas: CanvasDocumentSchema,
  ops: z.array(CanvasOpEnvelopeSchema),
  runs: z.array(z.unknown()),
});

export const CanvasEventsResponseSchema = z.object({
  ops: z.array(CanvasOpEnvelopeSchema),
});

export const CanvasPresenceResponseSchema = z.object({
  presence: z.object({
    canvas_id: z.string(),
    actor_id: z.string(),
    cursor_json: z.string().nullable().optional(),
    selection_json: z.string().nullable().optional(),
    viewport_json: z.string().nullable().optional(),
    updated_at: z.string(),
  }),
});

export const CanvasSocketEventSchema = z.object({
  type: z.enum(['op', 'presence']),
  canvas_id: z.string(),
  op: CanvasOpEnvelopeSchema.optional(),
  presence: z.unknown().optional(),
});

export type WorkbenchState = z.infer<typeof WorkbenchStateSchema>;
export type WorkspaceSummary = z.infer<typeof WorkspaceListSchema>['workspaces'][number];
export type RunEventEnvelope = z.infer<typeof RunEventEnvelopeSchema>;
export type RunStepState = z.infer<typeof RunStepStateSchema>;
export type RunStatus = z.infer<typeof RunStatusSchema>;
export type GraphNodeState = WorkbenchState['graph']['nodes'][number];
export type ProviderStatus = WorkbenchState['providers']['providers'][number];
export type CanvasActor = z.infer<typeof CanvasActorSchema>;
export type CanvasDocument = z.infer<typeof CanvasDocumentSchema>;
export type CanvasOpEnvelope = z.infer<typeof CanvasOpEnvelopeSchema>;
export type CanvasOpKind = z.infer<typeof CanvasOpKindSchema>;
export type CanvasSocketEvent = z.infer<typeof CanvasSocketEventSchema>;
