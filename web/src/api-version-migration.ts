import {
  ApplyVersionMigrationResponseSchema,
  type ApplyVersionMigrationResponse,
  VersionMigrationReportSchema,
  type VersionMigrationReport,
} from './version-migration-types';

export type ApplyVersionMigrationInput = {
  operationId: string;
  reportHash: string;
  sourceGraphHash: string;
  catalogRevision: string;
  workspaceConnectorId: string;
  migrationVersion: string;
};

export class VersionMigrationApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly code?: string,
  ) {
    super(message);
  }
}

export async function dryRunVersionMigration(
  workspaceId: string,
  versionId: string,
  signal?: AbortSignal,
): Promise<VersionMigrationReport> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/versions/${encodeURIComponent(versionId)}/migration/dry-run`,
    { method: 'POST', signal },
  );
  if (!response.ok) {
    throw new VersionMigrationApiError(
      response.status,
      `version migration dry-run failed: ${response.status}`,
    );
  }
  return VersionMigrationReportSchema.parse(await response.json());
}

export async function applyVersionMigration(
  workspaceId: string,
  versionId: string,
  input: ApplyVersionMigrationInput,
): Promise<ApplyVersionMigrationResponse> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/versions/${encodeURIComponent(versionId)}/migration/apply`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    },
  );
  if (!response.ok) {
    const body: unknown = await response.json().catch(() => null);
    const record = (body ?? {}) as { code?: unknown };
    const code = typeof record.code === 'string' ? record.code : undefined;
    throw new VersionMigrationApiError(
      response.status,
      response.status === 409
        ? '迁移前提已变化，请重新检查'
        : `version migration apply failed: ${response.status}`,
      code,
    );
  }
  return ApplyVersionMigrationResponseSchema.parse(await response.json());
}
