import { createRoot } from 'react-dom/client';
import '@xyflow/react/dist/style.css';
import '../src/styles.css';
import '../src/inspector.css';
import '../src/connections.css';
import '../src/node-library.css';

import { GraphCanvas } from '../src/components/graph-canvas';
import type {
  CanvasSnapshotUpdate,
  GraphNodeState,
  ManualProposalInput,
  NodeCatalog,
  WorkbenchState,
  WorkflowGraph,
} from '../src/types';

declare global {
  interface Window {
    __helixflowE2E: {
      logicalNodeCount: number;
      videoNodeCount: number;
      packVideos: boolean;
      proposals: ManualProposalInput[];
      snapshots: CanvasSnapshotUpdate[];
      rejectOps: string[];
    };
  }
}

const proposals: ManualProposalInput[] = [];
const snapshots: CanvasSnapshotUpdate[] = [];
const search = new URLSearchParams(location.search);
const requestedCount = Number.parseInt(search.get('nodes') ?? '2', 10);
const requestedVideos = Number.parseInt(search.get('videos') ?? '0', 10);
const logicalNodeCount = Number.isFinite(requestedCount) ? Math.max(2, requestedCount) : 2;
const videoNodeCount = Number.isFinite(requestedVideos) ? Math.max(0, requestedVideos) : 0;
const packVideos = search.get('pack') === '1';
const workspaceId = search.get('ws')?.trim() || 'ws_e2e';
const graph = graphFixture(logicalNodeCount, videoNodeCount, packVideos);
window.__helixflowE2E = {
  logicalNodeCount,
  videoNodeCount,
  packVideos,
  proposals,
  snapshots,
  rejectOps: [],
};

/** Tiny looping WebM so packed cards actually create decoders, not empty <video> tags. */
const SCALE_VIDEO_BASE64 =
  'GkXfo59ChoEBQveBAULygQRC84EIQoKEd2VibUKHgQRChYECGFOAZwEAAAAAAAHTEU2bdLpNu4tTq4QVSalmU6yBoU27i1OrhBZUrmtTrIHGTbuMU6uEElTDZ1OsggEXTbuMU6uEHFO7a1OsggHr7AEAAAAAAABZUrumz3KATYqF7oEBypXyggEX8ouOrhBA0qablW7j7/QFjk2bdLpNu4tTq4QWUOKydlu/gQKGhkFfTbuMU6uEHFO7a1OsggHr7AEAAAAAAABZUrumz3KATYqF7oEBypXyggEX8ouOrhBA0qablW7j7/QFjk2bdLpNu4tTq4QWUOKydlu/gQKGhkFfTbuMU6uEHFO7a1OsggHr7AEAAAAAAABZUrumz3KATYqF7oEBypXyggEX8ouOrhBA0qablW7j7/QFjg==';
const SCALE_VIDEO_DATA_URI = `data:video/webm;base64,${SCALE_VIDEO_BASE64}`;

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
  if (url.pathname.includes('/uploads/') && url.pathname.endsWith('/content')) {
    return new Response(scaleVideoBytes(), {
      status: 200,
      headers: { 'content-type': 'video/webm' },
    });
  }
  return nativeFetch(input, init);
};

createRoot(document.getElementById('root')!).render(
  <main className="canvas-e2e-harness">
    <GraphCanvas
      graph={graph}
      outputs={videoOutputs(graph.nodes)}
      workflowGraph={workflowFixture(graph.nodes)}
      pendingProposal={null}
      run={runFixture()}
      versionId="ver_e2e"
      workspaceId={workspaceId}
      onCreateProposal={async (proposal) => {
        proposals.push(proposal);
        const operation = proposal.ops[0]?.op;
        if (operation && window.__helixflowE2E.rejectOps.includes(operation)) {
          throw new Error(`${operation} rejected`);
        }
      }}
      onSaveCanvasSnapshot={async (update) => {
        snapshots.push(update);
        if (update.positions?.length && window.__helixflowE2E.rejectOps.includes('move_node')) {
          throw new Error('move_node rejected');
        }
        if (update.sizes?.length && window.__helixflowE2E.rejectOps.includes('resize_node')) {
          throw new Error('resize_node rejected');
        }
      }}
    />
  </main>,
);

function graphFixture(count: number, videos: number, pack: boolean): WorkbenchState['graph'] {
  const nodes: GraphNodeState[] = [
    node('text', 'input.text', 80, 100),
    node('video', 'video.text_to_video', 520, 100),
  ];
  const extraVideos = Math.min(Math.max(0, videos), Math.max(0, count - 2));
  for (let index = 2; index < count; index += 1) {
    const extraIndex = index - 2;
    const isVideo = extraIndex < extraVideos;
    const column = pack && isVideo ? extraIndex % 20 : index % 80;
    const row = pack && isVideo ? Math.floor(extraIndex / 20) : Math.floor(index / 80);
    const x = pack && isVideo ? column * 48 : column * 320;
    const y = pack && isVideo ? row * 48 : row * 180;
    nodes.push(node(
      isVideo ? `media-${index}` : `text-${index}`,
      isVideo ? 'input.video' : 'input.text',
      x,
      y,
    ));
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

function videoOutputs(nodes: GraphNodeState[]): WorkbenchState['outputs'] {
  return nodes
    .filter((item) => item.nodeType === 'input.video')
    .map((item) => ({
      id: `art_${item.id}`,
      kind: 'video',
      title: item.title,
      nodeId: item.id,
      storageUri: 'upload://scale-video',
      selected: false,
      meta: 'scale-fixture',
      mime: 'video/webm',
      preview: {
        kind: 'video',
        content: SCALE_VIDEO_DATA_URI,
        mime: 'video/webm',
      },
    }));
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
      {
        type: 'input.video',
        title: 'Video Input',
        category: 'input',
        provider: null,
        capability: null,
        description: 'Video scale fixture',
        inputs: [],
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

function workflowFixture(nodes: GraphNodeState[]): WorkflowGraph {
  return {
    schema_version: 1,
    nodes: Object.fromEntries(nodes.map((item) => [item.id, {
      node_type: item.nodeType,
      title: item.title,
      params: item.nodeType === 'input.video'
        ? { storage_uri: 'upload://scale-video' }
        : {},
      pos: [item.position.x, item.position.y] as [number, number],
    }])),
    edges: [],
  };
}

function scaleVideoBytes(): Uint8Array {
  const binary = atob(SCALE_VIDEO_BASE64);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}
