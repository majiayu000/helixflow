import { describe, expect, it } from 'vitest';
import {
  applyGroupSelection,
  applyUngroupSelection,
  canGroupSelectedNodes,
  expandSelectionWithGroups,
  groupFrames,
} from './graph-canvas-groups';
import type { GraphNodeState } from '../types';

describe('canvas groups', () => {
  it('requires two selected nodes to group', () => {
    expect(canGroupSelectedNodes(new Set(['a']))).toBe(false);
    expect(canGroupSelectedNodes(new Set(['a', 'b']))).toBe(true);
  });

  it('expands a grouped member into the full group', () => {
    const groups = applyGroupSelection(new Set(['a', 'b']), []);
    expect(expandSelectionWithGroups(new Set(['a']), groups)).toEqual(new Set(['a', 'b']));
  });

  it('ungroups members and drops empty frames', () => {
    const grouped = applyGroupSelection(new Set(['a', 'b']), []);
    expect(applyUngroupSelection(new Set(['a']), grouped)).toEqual([]);
  });

  it('builds a padded frame around live members', () => {
    const groups = applyGroupSelection(new Set(['a', 'b']), []);
    const frames = groupFrames(groups, [node('a', 0, 0), node('b', 100, 40)]);
    expect(frames).toHaveLength(1);
    expect(frames[0]!.width).toBeGreaterThan(100);
    expect(frames[0]!.height).toBeGreaterThan(40);
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
