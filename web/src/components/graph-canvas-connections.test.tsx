import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import type { GraphNodeState, NodeDefinition, WorkbenchState } from '../types';
import {
  buildConnectionProposalInput,
  buildPortHighlights,
  edgeToRemoveOp,
  portKey,
} from './graph-canvas-connections';
import { WorkflowNode } from './graph-canvas-node';

describe('graph canvas connections', () => {
  it('marks compatible, occupied, incompatible, and source ports', () => {
    const nodes = [textNode(), videoNode(), outputNode()];
    const definitions = new Map([
      ['input.text', textDefinition()],
      ['video.text_to_video', videoDefinition()],
      ['output.save', outputDefinition()],
    ]);
    const highlights = buildPortHighlights(
      nodes,
      definitions,
      { nodeId: 'text', port: 'text', type: 'TEXT' },
      [textToVideoEdge()],
    );

    expect(highlights.get(portKey('text', 'output', 'text'))).toBe('source');
    expect(highlights.get(portKey('video', 'input', 'prompt'))).toBe('occupied');
    expect(highlights.get(portKey('out', 'input', 'artifact'))).toBe('incompatible');
  });

  it('builds add, replace, no-op, and disconnect ops', () => {
    expect(
      buildConnectionProposalInput({
        baseVersionId: 'ver_1',
        source: { nodeId: 'text', port: 'text', type: 'TEXT' },
        target: { nodeId: 'writer', port: 'text', type: 'TEXT' },
      })?.ops,
    ).toEqual([
      { op: 'add_edge', from: ['text', 'text'], to: ['writer', 'text'], edge_type: 'text' },
    ]);

    expect(
      buildConnectionProposalInput({
        baseVersionId: 'ver_1',
        source: { nodeId: 'alt', port: 'text', type: 'TEXT' },
        target: { nodeId: 'video', port: 'prompt', type: 'TEXT' },
        existingEdge: textToVideoEdge(),
      })?.ops,
    ).toEqual([
      {
        op: 'remove_edge',
        from: ['text', 'text'],
        to: ['video', 'prompt'],
        edge_type: 'text',
      },
      { op: 'add_edge', from: ['alt', 'text'], to: ['video', 'prompt'], edge_type: 'text' },
    ]);

    expect(
      buildConnectionProposalInput({
        baseVersionId: 'ver_1',
        source: { nodeId: 'text', port: 'text', type: 'TEXT' },
        target: { nodeId: 'out', port: 'artifact', type: 'JSON' },
      }),
    ).toBeNull();
    expect(edgeToRemoveOp(textToVideoEdge())).toEqual({
      op: 'remove_edge',
      from: ['text', 'text'],
      to: ['video', 'prompt'],
      edge_type: 'text',
    });
  });

  it('renders catalog ports as hit targets with highlight classes', () => {
    const highlights = new Map([[portKey('video', 'input', 'prompt'), 'occupied' as const]]);
    const markup = renderToStaticMarkup(
      <WorkflowNode
        connectionDisabled={false}
        definition={videoDefinition()}
        diffState={null}
        dirty={false}
        locked={false}
        node={videoNode()}
        portHighlights={highlights}
        selected={false}
        stepState="queued"
        onOutputPortPointerDown={() => undefined}
        onPointerCancel={() => undefined}
        onPointerDown={() => undefined}
        onPointerMove={() => undefined}
        onPointerUp={() => undefined}
      />,
    );

    expect(markup).toContain('data-port-node-id="video"');
    expect(markup).toContain('data-port-direction="input"');
    expect(markup).toContain('data-port-direction="output"');
    expect(markup).toContain('port-target--occupied');
  });
});

function textToVideoEdge(): WorkbenchState['graph']['edges'][number] {
  return {
    id: 'edge_text_video',
    from: { nodeId: 'text', port: 'text' },
    to: { nodeId: 'video', port: 'prompt' },
    kind: 'text',
  };
}

function textNode(): GraphNodeState {
  return node('text', 'input.text', 'Text input', 'Input', 40, 80);
}

function videoNode(): GraphNodeState {
  return node('video', 'video.text_to_video', 'Video render', 'Video', 320, 80);
}

function outputNode(): GraphNodeState {
  return node('out', 'output.save', 'Save output', 'Output', 620, 80);
}

function node(
  id: string,
  nodeType: string,
  title: string,
  category: string,
  x: number,
  y: number,
): GraphNodeState {
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

function textDefinition(): NodeDefinition {
  return definition('input.text', 'Text input', [], [{ name: 'text', type: 'TEXT', required: true }]);
}

function videoDefinition(): NodeDefinition {
  return definition(
    'video.text_to_video',
    'Video render',
    [{ name: 'prompt', type: 'TEXT', required: true }],
    [{ name: 'video', type: 'VIDEO', required: true }],
  );
}

function outputDefinition(): NodeDefinition {
  return definition('output.save', 'Save output', [{ name: 'artifact', type: 'JSON', required: true }], []);
}

function definition(
  type: string,
  title: string,
  inputs: NodeDefinition['inputs'],
  outputs: NodeDefinition['outputs'],
): NodeDefinition {
  return {
    type,
    title,
    category: type.split('.')[0] ?? 'node',
    provider: null,
    capability: null,
    description: title,
    inputs,
    outputs,
    params_schema: {
      required: [],
      properties: {},
      allow_unknown: false,
    },
    estimated_cost: null,
  };
}
