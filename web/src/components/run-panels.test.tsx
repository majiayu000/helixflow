import { renderToStaticMarkup } from 'react-dom/server';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { dryRunVersionMigration } from '../api';
import { HistoryPanel, OutputsStrip } from './run-panels';
import type { WorkbenchState } from '../types';

vi.mock('../api', () => ({
  applyVersionMigration: vi.fn(),
  dryRunVersionMigration: vi.fn(),
}));

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

describe('HistoryPanel version migration', () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(async () => {
    if (renderer) await act(async () => renderer?.unmount());
    renderer = null;
    vi.clearAllMocks();
  });

  it('discards a completed report when the connector changes', async () => {
    vi.mocked(dryRunVersionMigration).mockResolvedValue({
      status: 'migratable',
      migrationVersion: '1',
      workspaceId: 'ws_a',
      sourceVersionId: 'ver_a',
      sourceGraphHash: 'sha256:source',
      sourceSchemaVersion: 1,
      catalogRevision: 'catalog-1',
      workspaceConnectorId: 'atlas',
      applyEnabled: true,
      reportHash: 'sha256:report',
      nodes: [],
    });
    await act(async () => {
      renderer = create(historyPanel('atlas'));
    });
    const view = renderer;
    if (!view) throw new Error('renderer was not created');
    const check = view.root
      .findAllByType('button')
      .find((button) => button.children.includes('检查'));
    await act(async () => {
      check?.props.onClick();
      await Promise.resolve();
    });
    expect(JSON.stringify(view.toJSON())).toContain('可以迁移');

    await act(async () => view.update(historyPanel('fal')));
    expect(JSON.stringify(view.toJSON())).not.toContain('可以迁移');
    expect(JSON.stringify(view.toJSON())).toContain('先检查当前版本');
  });
});

function historyPanel(connectorId: string) {
  return (
    <HistoryPanel
      busy={false}
      currentConnectorId={connectorId}
      currentVersionId="ver_a"
      currentWorkspaceId="ws_a"
      history={[]}
      onClose={noop}
      onMigrationApplied={noop}
      onOpenWorkspace={noop}
      onRestoreVersion={noop}
      open
      workspaceListError={null}
      workspaces={[]}
    />
  );
}
