import { describe, expect, it } from 'vitest';
import type { NodeDefinition, WorkbenchState, WorkflowGraph } from '../types';
import {
  buildAddNodeProposalInput,
  buildDeleteNodesProposalInput,
  buildPasteProposalInput,
  buildSelectionClipboardText,
  defaultParamsForDefinition,
  uniqueNodeId,
} from './graph-canvas-editing';

describe('graph canvas editing helpers', () => {
  it('builds add-node inputs with default params and unique ids', () => {
    const input = buildAddNodeProposalInput({
      baseVersionId: 'ver_1',
      definition: videoDefinition(),
      existingNodeIds: ['video_text_to_video_abc123'],
      position: { x: 220, y: 120 },
      suffix: 'abc123',
    });

    expect(input.ops).toEqual([{
      op: 'add_node',
      id: 'video_text_to_video_abc123_2',
      node_type: 'video.text_to_video',
      title: 'Text To Video',
      params: { prompt: '', duration_sec: 1, aspect_ratio: '1:1' },
      pos: [220, 120],
    }]);
    expect(defaultParamsForDefinition(numberDefinition())).toEqual({ seed: 0 });
  });

  it('deletes selected nodes as one batch and lets GraphService remove edges', () => {
    expect(
      buildDeleteNodesProposalInput({
        baseVersionId: 'ver_1',
        selectedNodeIds: ['video', 'text'],
      })?.ops,
    ).toEqual([
      { op: 'remove_node', id: 'text' },
      { op: 'remove_node', id: 'video' },
    ]);
  });

  it('copies and pastes an internal subgraph with rewritten ids', () => {
    const text = buildSelectionClipboardText({
      sourceVersionId: 'ver_1',
      nodes: [textNode(), videoNode()],
      edges: [textToVideoEdge()],
      workflowGraph: workflowGraph(),
    });
    const input = buildPasteProposalInput({
      baseVersionId: 'ver_2',
      existingNodeIds: ['text', 'video'],
      text,
      position: { x: 500, y: 300 },
    });

    expect(input?.ops).toEqual([
      {
        op: 'add_node',
        id: 'text_2',
        node_type: 'input.text',
        title: 'Text input',
        params: { text: 'hello' },
        pos: [500, 300],
      },
      {
        op: 'add_node',
        id: 'video_2',
        node_type: 'video.text_to_video',
        title: 'Video render',
        params: { prompt: '', duration_sec: 4, aspect_ratio: '1:1' },
        pos: [780, 300],
      },
      {
        op: 'add_edge',
        from: ['text_2', 'text'],
        to: ['video_2', 'prompt'],
        edge_type: 'text',
      },
    ]);
    expect(buildPasteProposalInput({
      baseVersionId: 'ver_2',
      existingNodeIds: [],
      text: '{"kind":"wrong"}',
      position: { x: 0, y: 0 },
    })).toBeNull();
  });

  it('generates deterministic unique ids from existing ids', () => {
    expect(uniqueNodeId(['node', 'node_2'], 'node')).toBe('node_3');
  });
});

function textNode(): WorkbenchState['graph']['nodes'][number] {
  return node('text', 'input.text', 'Text input', 'Input', 40, 80);
}

function videoNode(): WorkbenchState['graph']['nodes'][number] {
  return node('video', 'video.text_to_video', 'Video render', 'Video', 320, 80);
}

function node(
  id: string,
  nodeType: string,
  title: string,
  category: string,
  x: number,
  y: number,
): WorkbenchState['graph']['nodes'][number] {
  return {
    id,
    nodeType,
    title,
    category,
    status: 'queued',
    position: { x, y },
    provider: null,
    summary: '{}',
  };
}

function textToVideoEdge(): WorkbenchState['graph']['edges'][number] {
  return {
    id: 'edge_text_video',
    from: { nodeId: 'text', port: 'text' },
    to: { nodeId: 'video', port: 'prompt' },
    kind: 'text',
  };
}

function workflowGraph(): WorkflowGraph {
  return {
    schema_version: 1,
    nodes: {
      text: {
        node_type: 'input.text',
        title: 'Text input',
        params: { text: 'hello' },
        pos: [40, 80],
      },
      video: {
        node_type: 'video.text_to_video',
        title: 'Video render',
        params: { prompt: '', duration_sec: 4, aspect_ratio: '1:1' },
        pos: [320, 80],
      },
    },
    edges: [{ from: ['text', 'text'], to: ['video', 'prompt'], edge_type: 'text' }],
  };
}

function videoDefinition(): NodeDefinition {
  return definition(
    'video.text_to_video',
    'Text To Video',
    [
      ['prompt', { type: 'string', enum_values: [], minimum: null, maximum: null }],
      ['duration_sec', { type: 'integer', enum_values: [], minimum: 1, maximum: 10 }],
      ['aspect_ratio', { type: 'string', enum_values: ['1:1', '9:16'], minimum: null, maximum: null }],
    ],
  );
}

function numberDefinition(): NodeDefinition {
  return definition('mock.seed', 'Seed', [
    ['seed', { type: 'integer', enum_values: [], minimum: null, maximum: null }],
  ]);
}

function definition(
  type: string,
  title: string,
  properties: Array<[string, NodeDefinition['params_schema']['properties'][string]]>,
): NodeDefinition {
  return {
    type,
    title,
    category: type.split('.')[0] ?? 'node',
    provider: null,
    capability: null,
    description: title,
    inputs: [],
    outputs: [],
    params_schema: {
      required: properties.map(([key]) => key),
      properties: Object.fromEntries(properties),
      allow_unknown: false,
    },
    estimated_cost: null,
  };
}
