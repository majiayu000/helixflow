import { describe, expect, it, vi } from 'vitest';
import { MEDIA_CARD_HEIGHT, MEDIA_CARD_WIDTH } from '../graph-canvas-navigation';
import { selectedCanvasNodes, toWorkflowFlowEdges, toWorkflowFlowNodes } from './adapter';
import type { WorkflowFlowNode } from './types';

describe('toWorkflowFlowNodes', () => {
  it('exposes width and height so NodeResizer can stretch the card', () => {
    const nodes = toWorkflowFlowNodes({
      nodes: [
        {
          id: 'photo',
          nodeType: 'input.image',
          title: '图片',
          category: 'Input',
          status: 'queued',
          position: { x: 80, y: 80 },
          provider: null,
          summary: '{}',
        },
      ],
      baseComparableById: new Map(),
      definitionByType: new Map(),
      outputsByNodeId: new Map(),
      stepStateByNodeId: new Map(),
      hasProposal: false,
      canMove: true,
      canResize: true,
      onResizeCommit: vi.fn(),
    });

    expect(nodes[0]).toMatchObject({
      width: MEDIA_CARD_WIDTH,
      height: MEDIA_CARD_HEIGHT,
    } satisfies Partial<WorkflowFlowNode>);
  });
});

describe('toWorkflowFlowEdges', () => {
  it('adapts lineage edges without waiting for the node catalog', () => {
    expect(toWorkflowFlowEdges(
      [{
        id: 'e1',
        from: { nodeId: 'photo', port: 'image' },
        to: { nodeId: 'image_generate', port: 'in' },
        kind: 'image',
      }],
      new Set(),
      false,
    )).toEqual([expect.objectContaining({
      id: 'e1',
      source: 'photo',
      sourceHandle: 'image',
      target: 'image_generate',
      targetHandle: 'in',
      type: 'card',
    })]);
  });

  it('keeps duplicate lineage edges distinct by graph edge id', () => {
    const edges = toWorkflowFlowEdges(
      [
        {
          id: 'edge_photo_image_tile_in_0',
          from: { nodeId: 'photo', port: 'image' },
          to: { nodeId: 'tile', port: 'in' },
          kind: 'image',
        },
        {
          id: 'edge_photo_image_tile_in_1',
          from: { nodeId: 'photo', port: 'image' },
          to: { nodeId: 'tile', port: 'in' },
          kind: 'image',
        },
      ],
      new Set(),
      false,
    );
    expect(edges.map((edge) => edge.id)).toEqual([
      'edge_photo_image_tile_in_0',
      'edge_photo_image_tile_in_1',
    ]);
  });
});

describe('selectedCanvasNodes', () => {
  it('keeps overlay geometry on the live dragged card', () => {
    const selected = selectedCanvasNodes(
      [{
        id: 'photo',
        nodeType: 'input.image',
        title: '图片',
        category: 'Input',
        status: 'queued',
        position: { x: 80, y: 80 },
        provider: null,
        summary: '{}',
      }],
      new Set(['photo']),
      [{ id: 'photo', position: { x: 240, y: 120 }, width: 300, height: 220 }],
    );
    expect(selected).toEqual([expect.objectContaining({
      id: 'photo',
      position: { x: 240, y: 120 },
      size: { width: 300, height: 220 },
    })]);
  });
});
