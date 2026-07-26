import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { VersionMigrationPanel } from './components/version-migration-panel';
import { useVersionMigrationStore } from './store-version-migration';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
import { jsonResponse } from './test-utils';
import type { VersionMigrationReport } from './version-migration-types';
import type { WorkbenchState } from './types';

const context = {
  workspaceId: 'ws_1',
  versionId: 'ver_v1',
  connectorId: 'atlas',
};

afterEach(() => {
  useVersionMigrationStore.getState().setContext(null);
  useVersionMigrationStore.setState({
    phase: 'idle',
    report: null,
    error: null,
    successVersionId: null,
    pendingApply: null,
  });
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('version migration store', () => {
  it('retries an unknown apply result with the same operation ID', async () => {
    const applyBodies: Array<Record<string, unknown>> = [];
    let applyAttempt = 0;
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input).endsWith('/dry-run')) return jsonResponse(report());
      applyBodies.push(JSON.parse(String(init?.body)));
      applyAttempt += 1;
      if (applyAttempt === 1) throw new TypeError('connection lost');
      return jsonResponse(applyResponse());
    }));
    const store = useVersionMigrationStore.getState();
    store.setContext(context);
    await store.inspect();
    await useVersionMigrationStore.getState().apply(() => undefined);

    const unknown = useVersionMigrationStore.getState();
    expect(unknown.phase).toBe('unknown');
    expect(unknown.pendingApply?.status).toBe('unknown');
    await unknown.apply(() => undefined);

    expect(applyBodies).toHaveLength(2);
    expect(applyBodies[1]?.operationId).toBe(applyBodies[0]?.operationId);
    expect(useVersionMigrationStore.getState().phase).toBe('success');
    expect(useVersionMigrationStore.getState().pendingApply).toBeNull();
  });

  it('keeps an unknown operation while another workspace is visible', async () => {
    let rejectApply!: (reason?: unknown) => void;
    vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
      if (String(input).endsWith('/dry-run')) {
        return Promise.resolve(jsonResponse(report()));
      }
      return new Promise<Response>((_resolve, reject) => {
        rejectApply = reject;
      });
    }));
    const store = useVersionMigrationStore.getState();
    store.setContext(context);
    await store.inspect();
    const applying = useVersionMigrationStore.getState().apply(() => undefined);
    const operationId = useVersionMigrationStore.getState().pendingApply?.operationId;

    useVersionMigrationStore.getState().setContext({
      workspaceId: 'ws_2',
      versionId: 'ver_2',
      connectorId: 'fal',
    });
    expect(useVersionMigrationStore.getState().phase).toBe('pending_elsewhere');
    rejectApply(new TypeError('connection lost'));
    await applying;
    expect(useVersionMigrationStore.getState().pendingApply?.operationId).toBe(operationId);

    useVersionMigrationStore.getState().setContext(context);
    expect(useVersionMigrationStore.getState().phase).toBe('unknown');
  });

  it('aborts and discards a stale dry-run when connector context changes', async () => {
    let requestSignal: AbortSignal | undefined;
    vi.stubGlobal('fetch', vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
      requestSignal = init?.signal ?? undefined;
      return new Promise<Response>((_resolve, reject) => {
        requestSignal?.addEventListener('abort', () => {
          reject(new DOMException('aborted', 'AbortError'));
        });
      });
    }));
    const store = useVersionMigrationStore.getState();
    store.setContext(context);
    const checking = useVersionMigrationStore.getState().inspect();
    useVersionMigrationStore.getState().setContext({ ...context, connectorId: 'fal' });
    await checking;

    expect(requestSignal?.aborted).toBe(true);
    expect(useVersionMigrationStore.getState().report).toBeNull();
    expect(useVersionMigrationStore.getState().phase).toBe('idle');
  });

  it('clears stale report and operation after a definite conflict', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input).endsWith('/dry-run')) return jsonResponse(report());
      return new Response(JSON.stringify({ error: 'stale' }), { status: 409 });
    }));
    const store = useVersionMigrationStore.getState();
    store.setContext(context);
    await store.inspect();
    await useVersionMigrationStore.getState().apply(() => undefined);

    expect(useVersionMigrationStore.getState()).toMatchObject({
      phase: 'conflict',
      pendingApply: null,
      report: null,
      error: '迁移前提已变化，请重新检查',
    });
  });
});

describe('VersionMigrationPanel', () => {
  it('uses server applyEnabled and exposes live status accessibly', async () => {
    const view = await mountPanel();
    await act(async () => {
      useVersionMigrationStore.setState({
        phase: 'ready',
        report: report({ applyEnabled: false }),
        error: null,
      });
    });
    const markup = JSON.stringify(view.toJSON());
    expect(markup).toContain('"aria-live":"polite"');
    expect(markup).toContain('服务端尚未开放 apply');
    expect(view.root.findByType('button').props.disabled).toBe(true);
    await act(async () => view.unmount());
  });

  it('renders top-level failures separately from located node failures', async () => {
    const view = await mountPanel();
    await act(async () => {
      useVersionMigrationStore.setState({
        phase: 'ready',
        report: report({
          status: 'needs_resolution',
          nodes: [{
            nodeId: 'image',
            action: 'needs_resolution',
            code: 'MODEL_AMBIGUOUS',
            message: 'multiple models',
            candidates: ['model-a', 'model-b'],
          }],
        }),
        error: null,
      });
    });
    const markup = JSON.stringify(view.toJSON());
    expect(markup).toContain('"aria-label":"迁移节点 image"');
    expect(markup).toContain('MODEL_AMBIGUOUS');

    await act(async () => {
      useVersionMigrationStore.setState({
        report: report({
          status: 'failed',
          code: 'SOURCE_GRAPH_INVALID',
          message: 'source graph payload is invalid',
          nodes: [],
        }),
      });
    });
    const failed = JSON.stringify(view.toJSON());
    expect(failed).toContain('SOURCE_GRAPH_INVALID');
    expect(failed).not.toContain('迁移节点');
    await act(async () => view.unmount());
  });
});

function report(overrides: Partial<VersionMigrationReport> = {}): VersionMigrationReport {
  return {
    status: 'migratable',
    migrationVersion: '1',
    workspaceId: 'ws_1',
    sourceVersionId: 'ver_v1',
    sourceGraphHash: 'sha256:source',
    sourceSchemaVersion: 1,
    catalogRevision: 'catalog-1',
    workspaceConnectorId: 'atlas',
    applyEnabled: true,
    reportHash: 'sha256:report',
    nodes: [],
    ...overrides,
  };
}

function applyResponse() {
  return {
    targetVersionId: 'ver_v2',
    targetGraphHash: 'sha256:target',
    replayed: false,
    workspaceState: emptyWorkbenchState(),
  };
}

function emptyWorkbenchState(): WorkbenchState {
  return {
    eventSeq: 0,
    workspace: {
      id: 'ws_1',
      name: 'Migration',
      versionId: 'ver_v2',
      updatedAt: '2026-07-27T00:00:00Z',
    },
    providers: {
      defaultProvider: 'atlas',
      selectedProvider: 'atlas',
      runtimeProviders: [],
      workflowBackends: [],
      apiConnectors: [],
    },
    chat: { messages: [] },
    graph: { nodes: [], edges: [] },
    run: null,
    outputs: [],
    history: [],
    pendingConfirmation: null,
    pendingProposal: null,
  };
}

async function mountPanel(): Promise<ReactTestRenderer> {
  let view!: ReactTestRenderer;
  await act(async () => {
    view = create(
      <VersionMigrationPanel
        busy={false}
        connectorId="atlas"
        onApplied={() => undefined}
        open
        versionId="ver_v1"
        workspaceId="ws_1"
      />,
    );
  });
  return view;
}
