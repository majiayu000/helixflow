import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { HistoryPanel, OutputsStrip } from './run-panels';
import type { WorkbenchState } from '../types';

type Output = WorkbenchState['outputs'][number];

function output(overrides: Partial<Output>): Output {
  return {
    id: 'art_1',
    kind: 'video',
    title: 'Clip',
    storageUri: '/api/artifacts/art_1/content',
    selected: false,
    meta: '',
    ...overrides,
  };
}

function noop() {}

describe('OutputsStrip review controls', () => {
  it('shows accept and reject buttons for a pending output', () => {
    const markup = renderToStaticMarkup(
      <OutputsStrip
        outputs={[output({ reviewState: 'pending' })]}
        busy={false}
        onSelect={noop}
        onAccept={noop}
        onReject={noop}
      />,
    );
    expect(markup).toContain('output-review-accept');
    expect(markup).toContain('output-review-reject');
    expect(markup).not.toContain('已接受');
  });

  it('shows a settled label instead of buttons for an accepted output', () => {
    const markup = renderToStaticMarkup(
      <OutputsStrip
        outputs={[output({ reviewState: 'accepted' })]}
        busy={false}
        onSelect={noop}
        onAccept={noop}
        onReject={noop}
      />,
    );
    expect(markup).toContain('已接受');
    expect(markup).not.toContain('output-review-accept');
  });

  it('treats a missing reviewState as pending', () => {
    const markup = renderToStaticMarkup(
      <OutputsStrip
        outputs={[output({})]}
        busy={false}
        onSelect={noop}
        onAccept={noop}
        onReject={noop}
      />,
    );
    expect(markup).toContain('output-review-accept');
  });
});

describe('HistoryPanel navigation lock', () => {
  it('disables workspace navigation while a guarded action is pending', () => {
    const markup = renderToStaticMarkup(
      <HistoryPanel
        busy
        currentVersionId="ver_a"
        currentWorkspaceId="ws_a"
        currentConnectorId="atlas"
        history={[]}
        onClose={noop}
        onOpenWorkspace={noop}
        onRestoreVersion={noop}
        onMigrationApplied={noop}
        open
        workspaceListError={null}
        workspaces={[
          { id: 'ws_a', name: 'A', versionId: 'ver_a', createdAt: '', updatedAt: 'unix:1', firstMessage: null, messageCount: 0 },
          { id: 'ws_b', name: 'B', versionId: 'ver_b', createdAt: '', updatedAt: 'unix:2', firstMessage: null, messageCount: 0 },
        ]}
      />,
    );

    expect(markup.match(/class="workspace-history-row[^"]*" disabled=""/g)).toHaveLength(2);
  });
});

describe('HistoryPanel version source badge', () => {
  it('labels version entries by structured source and skips entries without one', () => {
    const markup = renderToStaticMarkup(
      <HistoryPanel
        busy={false}
        currentConnectorId="atlas"
        currentVersionId="ver_a"
        currentWorkspaceId="ws_a"
        history={[
          { id: 'ver_a', kind: 'version', label: 'Base', time: 'unix:1', summary: 'manual graph', source: 'manual' },
          { id: 'ver_b', kind: 'version', label: 'Agent edit', time: 'unix:2', summary: 'proposal graph', source: 'proposal' },
          { id: 'ver_c', kind: 'version', label: 'Rollback', time: 'unix:3', summary: 'restore graph', source: 'restore' },
          { id: 'run_a', kind: 'run', label: 'Run', time: 'unix:4', summary: 'succeeded' },
        ]}
        onClose={noop}
        onOpenWorkspace={noop}
        onMigrationApplied={noop}
        onRestoreVersion={noop}
        open
        workspaceListError={null}
        workspaces={[]}
      />,
    );

    expect(markup).toContain('data-source="manual"');
    expect(markup).toContain('data-source="proposal"');
    expect(markup).toContain('data-source="restore"');
    expect(markup).toContain('>手动<');
    expect(markup).toContain('>Agent<');
    expect(markup).toContain('>回滚<');
    expect(markup.match(/history-source/g)).toHaveLength(3);
  });
});
