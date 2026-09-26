import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import type { GraphNodeState, NodeDefinition, WorkbenchState } from '../types';
import {
  buildConnectionProposalInput,
  buildPortHighlights,
  canvasCardFallbackPorts,
  edgeToRemoveOp,
  incomingReferenceBag,
  lineageConnection,
  matchingConnectionPorts,
  portKey,
  referenceBagLabel,
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

  it('keeps many-cardinality image ports connectable after the first edge', () => {
    const nodes = [
      node('still_a', 'input.image', 'Still A', 'Input', 40, 80),
      node('still_b', 'input.image', 'Still B', 'Input', 40, 200),
      node('r2v', 'video.image_to_video', 'R2V', 'Video', 400, 80),
    ];
    const definitions = new Map([
      ['input.image', definition('input.image', 'Image Input', [], [{ name: 'image', type: 'IMAGE', required: true }])],
      [
        'video.image_to_video',
        definition(
          'video.image_to_video',
          'Image To Video',
          [{ name: 'image', type: 'IMAGE', required: true, cardinality: 'many' }],
          [{ name: 'video', type: 'VIDEO', required: true }],
        ),
      ],
    ]);
    const highlights = buildPortHighlights(
      nodes,
      definitions,
      { nodeId: 'still_b', port: 'image', type: 'IMAGE' },
      [{
        id: 'e1',
        from: { nodeId: 'still_a', port: 'image' },
        to: { nodeId: 'r2v', port: 'image' },
        kind: 'image',
      }],
    );
    expect(highlights.get(portKey('r2v', 'input', 'image'))).toBe('compatible');
  });

  it('gives canvas cards fallback in/out handles before the catalog loads', () => {
    expect(canvasCardFallbackPorts('input.image')).toEqual({
      inputs: [{ name: 'in', type: 'IMAGE' }],
      outputs: [{ name: 'image', type: 'IMAGE' }],
    });
    expect(canvasCardFallbackPorts('image.generate')?.outputs[0]?.name).toBe('image');
    expect(canvasCardFallbackPorts('video.text_to_video')?.outputs[0]?.name).toBe('video');
    expect(canvasCardFallbackPorts('input.text')?.outputs[0]?.name).toBe('text');
    expect(canvasCardFallbackPorts('llm.prompt_writer')).toBeNull();
  });

  it('resolves lineage ports from node types instead of feature-specific handles', () => {
    expect(lineageConnection('input.image', 'image.generate')).toEqual({
      sourcePort: 'image',
      targetPort: 'in',
      type: 'IMAGE',
    });
    expect(lineageConnection('input.image', 'image.edit')).toEqual({
      sourcePort: 'image',
      targetPort: 'image',
      type: 'IMAGE',
    });
    expect(lineageConnection('image.generate', 'input.image')).toEqual({
      sourcePort: 'image',
      targetPort: 'in',
      type: 'IMAGE',
    });
    expect(lineageConnection('input.image', 'input.image')).toEqual({
      sourcePort: 'image',
      targetPort: 'in',
      type: 'IMAGE',
    });
    expect(() => lineageConnection('input.text', 'llm.prompt_writer')).toThrow(
      '连线失败：这两张卡没有可接的端口',
    );
  });

  it('connects two image cards through the reference port', () => {
    const imageDef = definition(
      'input.image',
      'Image Input',
      [{ name: 'in', type: 'IMAGE', required: false, cardinality: 'many' }],
      [{ name: 'image', type: 'IMAGE', required: true }],
    );
    const proposal = buildConnectionProposalInput({
      baseVersionId: 'ver_1',
      source: { nodeId: 'still_a', port: 'image', type: 'IMAGE' },
      target: { nodeId: 'still_b', port: 'in', type: 'IMAGE' },
      fanIn: true,
    });
    expect(proposal?.ops).toEqual([
      { op: 'add_edge', from: ['still_a', 'image'], to: ['still_b', 'in'], edge_type: 'image' },
    ]);
    expect(matchingConnectionPorts(imageDef, imageDef)).toEqual({
      sourcePort: 'image',
      targetPort: 'in',
      type: 'IMAGE',
    });
  });

  it('lets an image card reference a video card through in', () => {
    const imageDef = definition(
      'input.image',
      'Image Input',
      [{ name: 'in', type: 'IMAGE', required: false, cardinality: 'many' }],
      [{ name: 'image', type: 'IMAGE', required: true }],
    );
    const videoDef = definition(
      'input.video',
      'Video Input',
      [{ name: 'in', type: 'VIDEO', required: false, cardinality: 'many' }],
      [{ name: 'video', type: 'VIDEO', required: true }],
    );
    expect(matchingConnectionPorts(imageDef, videoDef)).toEqual({
      sourcePort: 'image',
      targetPort: 'in',
      type: 'IMAGE',
    });
    expect(
      buildConnectionProposalInput({
        baseVersionId: 'ver_1',
        source: { nodeId: 'still', port: 'image', type: 'IMAGE' },
        target: { nodeId: 'clip', port: 'in', type: 'VIDEO' },
        fanIn: true,
      })?.ops,
    ).toEqual([
      { op: 'add_edge', from: ['still', 'image'], to: ['clip', 'in'], edge_type: 'image' },
    ]);
  });

  it('counts incoming image video and audio edges as a reference bag', () => {
    const edges: WorkbenchState['graph']['edges'] = [
      {
        id: 'e1',
        from: { nodeId: 'still_a', port: 'image' },
        to: { nodeId: 'r2v', port: 'image' },
        kind: 'image',
      },
      {
        id: 'e2',
        from: { nodeId: 'still_b', port: 'image' },
        to: { nodeId: 'r2v', port: 'image' },
        kind: 'image',
      },
      {
        id: 'e3',
        from: { nodeId: 'clip', port: 'video' },
        to: { nodeId: 'r2v', port: 'video' },
        kind: 'video',
      },
    ];
    expect(incomingReferenceBag('r2v', edges)).toEqual({
      images: 2,
      videos: 1,
      audios: 0,
    });
    expect(referenceBagLabel(incomingReferenceBag('r2v', edges))).toBe('参考袋 2 图 · 1 视频');
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
    const highlights = new Map([[portKey('out', 'input', 'artifact'), 'occupied' as const]]);
    const markup = renderToStaticMarkup(
      <WorkflowNode
        connectionDisabled={false}
        definition={outputDefinition()}
        diffState={null}
        dirty={false}
        locked={false}
        node={outputNode()}
        artifactOutputs={[]}
        portHighlights={highlights}
        resizable={false}
        selected={false}
        stepState="queued"
        onOutputPortPointerDown={() => undefined}
        onPointerCancel={() => undefined}
        onPointerDown={() => undefined}
        onPointerMove={() => undefined}
        onPointerUp={() => undefined}
        onResizePointerCancel={() => undefined}
        onResizePointerDown={() => undefined}
        onResizePointerMove={() => undefined}
        onResizePointerUp={() => undefined}
      />,
    );

    expect(markup).toContain('data-port-node-id="out"');
    expect(markup).toContain('data-port-direction="input"');
    expect(markup).toContain('port-target--occupied');
  });

  it('renders media input as a card without node chrome', () => {
    const markup = renderToStaticMarkup(
      <WorkflowNode
        connectionDisabled={false}
        definition={definition(
          'input.image',
          'Image Input',
          [],
          [{ name: 'image', type: 'IMAGE', required: true }],
        )}
        diffState={null}
        dirty={false}
        locked={false}
        node={node('input_image', 'input.image', 'Image Input', 'Input', 40, 80)}
        artifactOutputs={[]}
        portHighlights={new Map()}
        resizable={false}
        selected={true}
        stepState="queued"
        onOutputPortPointerDown={() => undefined}
        onPointerCancel={() => undefined}
        onPointerDown={() => undefined}
        onPointerMove={() => undefined}
        onPointerUp={() => undefined}
        onResizePointerCancel={() => undefined}
        onResizePointerDown={() => undefined}
        onResizePointerMove={() => undefined}
        onResizePointerUp={() => undefined}
      />,
    );

    expect(markup).toContain('node--media');
    expect(markup).toContain('media-card--empty');
    expect(markup).toContain('media-card-icon');
    expect(markup).toContain('media-card-kind');
    expect(markup).toContain('图片');
    expect(markup).not.toContain('type="file"');
    expect(markup).toContain('aria-label="图片 (input.image)"');
    expect(markup).not.toContain('Image Input');
    expect(markup).not.toContain('点击或拖入文件');
    expect(markup).not.toContain('p-nid');
    expect(markup).not.toContain('io-row');
    expect(markup).not.toContain('SELECTED');
    expect(markup).not.toContain('class="swatch"');
  });

  it('renders video cards without native playback controls', () => {
    const markup = renderToStaticMarkup(
      <WorkflowNode
        connectionDisabled={false}
        definition={videoDefinition()}
        diffState={null}
        dirty={false}
        locked={false}
        node={videoNode()}
        artifactOutputs={[]}
        portHighlights={new Map()}
        resizable={false}
        selected={true}
        stepState="queued"
        onOutputPortPointerDown={() => undefined}
        onPointerCancel={() => undefined}
        onPointerDown={() => undefined}
        onPointerMove={() => undefined}
        onPointerUp={() => undefined}
        onResizePointerCancel={() => undefined}
        onResizePointerDown={() => undefined}
        onResizePointerMove={() => undefined}
        onResizePointerUp={() => undefined}
      />,
    );

    expect(markup).toContain('node--media');
    expect(markup).toContain('视频');
    expect(markup).not.toContain('controls');
    expect(markup).not.toContain('<video');
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
