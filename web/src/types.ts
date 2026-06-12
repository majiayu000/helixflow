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
  'running',
  'succeeded',
  'failed',
  'interrupted',
]);

const CostSchema = z.object({
  amount: z.number(),
  currency: z.string(),
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
    messages: z.array(
      z.object({
        id: z.string(),
        role: z.enum(['user', 'agent', 'system']),
        text: z.string(),
        time: z.string(),
      }),
    ),
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
      kind: z.enum(['text', 'image', 'video', 'json']),
      title: z.string(),
      storageUri: z.string(),
      selected: z.boolean(),
      meta: z.string(),
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
  pendingConfirmation: z
    .object({
      id: z.string(),
      title: z.string(),
      summary: z.string(),
      cost: CostSchema,
    })
    .nullable(),
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
