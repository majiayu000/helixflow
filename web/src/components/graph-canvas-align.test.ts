import { describe, expect, it } from 'vitest';
import { alignNodePositions } from './graph-canvas-align';
import type { GraphNodeState } from '../types';

describe('alignNodePositions', () => {
  it('aligns two nodes to the leftmost x', () => {
    const next = alignNodePositions([node('a', 80, 10), node('b', 20, 40)], 'left');
    expect(next).toEqual([{ id: 'a', x: 20, y: 10 }]);
  });

  it('does nothing for a single node', () => {
    expect(alignNodePositions([node('a', 10, 10)], 'left')).toEqual([]);
  });
});

function node(id: string, x: number, y: number): GraphNodeState {
  return {
    cached: false,
    category: 'Input',
    id,
    nodeType: 'input.text',
    position: { x, y },
    provider: null,
    status: 'queued',
    summary: '{}',
    title: id,
  };
}
