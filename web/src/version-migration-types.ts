import { z } from 'zod';
import { WorkbenchStateSchema } from './types';

export const VersionMigrationReasonCodeSchema = z.enum([
  'UNKNOWN_NODE_TYPE',
  'CAPABILITY_NOT_FOUND',
  'MODEL_NOT_FOUND',
  'MODEL_AMBIGUOUS',
  'MODEL_CAPABILITY_MISMATCH',
  'BINDING_NOT_FOUND',
  'BINDING_AMBIGUOUS',
  'DEFAULT_BINDING_MISSING',
  'SOURCE_GRAPH_MISSING',
  'SOURCE_GRAPH_HASH_MISMATCH',
  'SOURCE_GRAPH_INVALID',
  'SOURCE_GRAPH_STRUCTURAL_INVALID',
  'MIGRATED_GRAPH_INVALID',
  'SEMANTICS_INVALID',
  'WORKSPACE_CONNECTOR_INCOMPATIBLE',
]);

export const VersionMigrationReportSchema = z.object({
  status: z.enum(['migratable', 'needs_resolution', 'already_migrated', 'failed']),
  code: VersionMigrationReasonCodeSchema.optional(),
  message: z.string().optional(),
  migrationVersion: z.string(),
  workspaceId: z.string(),
  sourceVersionId: z.string(),
  sourceGraphHash: z.string(),
  sourceSchemaVersion: z.number(),
  catalogRevision: z.string(),
  workspaceConnectorId: z.string(),
  applyEnabled: z.boolean(),
  reportHash: z.string(),
  nodes: z.array(z.object({
    nodeId: z.string(),
    action: z.string(),
    code: VersionMigrationReasonCodeSchema.optional(),
    message: z.string().optional(),
    candidates: z.array(z.string()),
  })),
});

export const ApplyVersionMigrationResponseSchema = z.object({
  targetVersionId: z.string(),
  targetGraphHash: z.string(),
  replayed: z.boolean(),
  workspaceState: WorkbenchStateSchema,
});

export type VersionMigrationReport = z.infer<typeof VersionMigrationReportSchema>;
export type ApplyVersionMigrationResponse = z.infer<typeof ApplyVersionMigrationResponseSchema>;
