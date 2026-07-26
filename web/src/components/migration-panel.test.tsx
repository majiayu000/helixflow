import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { MigrationReportView } from './migration-panel';
import type { MigrationDryRun } from '../types';

function dryRun(overrides: Partial<MigrationDryRun>): MigrationDryRun {
  return {
    status: 'ready',
    versionId: 'ver_1',
    counts: { mapped: 1, structural: 1, needsResolution: 0 },
    report: {
      migrationVersion: '1',
      sourceSchemaVersion: 1,
      catalogRevision: 'sha256:test',
      resolvable: true,
      nodes: [
        { nodeId: 'input', action: 'structural' },
        { nodeId: 'image', action: { mappedPolicy: { capabilityId: 'text_to_image' } } },
      ],
    },
    ...overrides,
  };
}

describe('MigrationReportView', () => {
  it('renders counts and per-node actions', () => {
    const markup = renderToStaticMarkup(<MigrationReportView dryRun={dryRun({})} />);

    expect(markup).toContain('可迁移 1');
    expect(markup).toContain('需人工处理');
    expect(markup).toContain('text_to_image');
    expect(markup).toContain('结构节点');
  });

  it('surfaces needs-resolution nodes with their reasons', () => {
    const markup = renderToStaticMarkup(
      <MigrationReportView
        dryRun={dryRun({
          status: 'needsResolution',
          counts: { mapped: 0, structural: 0, needsResolution: 1 },
          report: {
            migrationVersion: '1',
            sourceSchemaVersion: 1,
            catalogRevision: 'sha256:test',
            resolvable: false,
            nodes: [
              {
                nodeId: 'image',
                action: { needsResolution: { reason: 'no catalog model matches `sora ultra`' } },
              },
            ],
          },
        })}
      />,
    );

    expect(markup).toContain('无法自动解析');
    expect(markup).toContain('sora ultra');
    expect(markup).toContain('image');
  });

  it('reports already-migrated versions as a no-op', () => {
    const markup = renderToStaticMarkup(
      <MigrationReportView dryRun={dryRun({ status: 'alreadyMigrated', report: undefined })} />,
    );
    expect(markup).toContain('已带语义层');
  });
});
