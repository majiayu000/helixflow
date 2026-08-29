import { createRoot } from 'react-dom/client';
import '@xyflow/react/dist/style.css';
import '../src/styles.css';
import '../src/inspector.css';
import '../src/connections.css';
import '../src/node-library.css';

import { GraphCanvas } from '../src/components/graph-canvas';
import type {
  GraphNodeState,
  ManualProposalInput,
  NodeCatalog,
  WorkbenchState,
} from '../src/types';

declare global {
  interface Window {
    __helixflowE2E: {
      logicalNodeCount: number;
      proposals: ManualProposalInput[];
    };
  }
}

const proposals: ManualProposalInput[] = [];
const requestedCount = Number.parseInt(new URLSearchParams(location.search).get('nodes') ?? '2', 10);
const logicalNodeCount = Number.isFinite(requestedCount) ? Math.max(2, requestedCount) : 2;
const graph = graphFixture(logicalNodeCount);
window.__helixflowE2E = { logicalNodeCount, proposals };

const nativeFetch = window.fetch.bind(window);
window.fetch = async (input, init) => {
  const url = new URL(input instanceof Request ? input.url : String(input), location.origin);
  if (url.pathname === '/api/registry/catalog') return jsonResponse(nodeCatalog());
  if (url.pathname === '/api/catalog') {
    return jsonResponse({
      catalogRevision: 'e2e',
      capabilities: [],
      models: [],
      bindings: [],
      defaultBindings: {},
    });
  }
  if (url.pathname.endsWith('/catalog/resolve')) {
    return new Response(JSON.stringify({
      error: 'No runtime provider is needed by this browser fixture',
      details: { code: 'PROVIDER_UNAVAILABLE', recoverable: true },
    }), {
      status: 409,
      headers: { 'content-type': 'application/json' },
    });
  }
  return nativeFetch(input, init);
};

createRoot(document.getElementById('root')!).render(
  <main className="canvas-e2e-harness">
    <GraphCanvas
      graph={graph}
      outputs={[]}
      pendingProposal={null}
      run={runFixture()}
      versionId="ver_e2e"
      workspaceId="ws_e2e"
      onCreateProposal={async (proposal) => {
        proposals.push(proposal);
      }}
    />
  </main>,
);

function graphFixture(count: number): WorkbenchState['graph'] {
  const nodes: GraphNodeState[] = [
    node('text', 'input.text', 80, 100),
    node('video', 'video.text_to_video', 520, 100),
  ];
  for (let index = 2; index < count; index += 1) {
    const column = index % 80;
    const row = Math.floor(index / 80);
    nodes.push(node(`text-${index}`, 'input.text', column * 320, row * 180));
  }
  return { nodes, edges: [] };
}

function node(id: string, nodeType: string, x: number, y: number): GraphNodeState {
  return {
    id,
    nodeType,
    title: id,
    category: nodeType.startsWith('video.') ? 'Video' : 'Input',
    status: 'queued',
    position: { x, y },
    provider: nodeType.startsWith('video.') ? 'mock' : null,
    summary: nodeType,
  };
}

function nodeCatalog(): NodeCatalog {
  const params_schema = { required: [], properties: {}, allow_unknown: true };
  return {
    schema_version: 1,
    nodes: [
      {
        type: 'input.text',
        title: 'Text Input',
        category: 'input',
        provider: null,
        capability: null,
        description: 'Text fixture',
        inputs: [],
        outputs: [{ name: 'text', type: 'TEXT', required: true }],
        params_schema,
        estimated_cost: null,
      },
      {
        type: 'video.text_to_video',
        title: 'Text To Video',
        category: 'video',
        provider: 'mock',
        capability: 'text_to_video',
        description: 'Video fixture',
        inputs: [{ name: 'prompt', type: 'TEXT', required: true }],
        outputs: [{ name: 'video', type: 'VIDEO', required: true }],
        params_schema,
        estimated_cost: null,
      },
    ],
  };
}

function runFixture(): NonNullable<WorkbenchState['run']> {
  return {
    id: 'run_e2e',
    label: 'Browser fixture',
    status: 'queued',
    steps: [],
    cost: { estimate: 0, actual: 0, currency: 'USD' },
  };
}

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}
