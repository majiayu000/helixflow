import { describe, expect, it } from 'vitest';
import { isRelatedNeighbor, relatedHighlight } from './graph-canvas-relations';
import type { WorkbenchState } from '../types';

describe('relatedHighlight', () => {
  const edges: WorkbenchState['graph']['edges'] = [
    { id: 'e1', kind: 'image', from: { nodeId: 'a', port: 'out' }, to: { nodeId: 'b', port: 'in' } },
    { id: 'e2', kind: 'image', from: { nodeId: 'b', port: 'out' }, to: { nodeId: 'c', port: 'in' } },
  ];

  it('returns empty sets without an active node', () => {
    expect(relatedHighlight(null, edges)).toEqual({ nodeIds: new Set(), edgeIds: new Set() });
  });

  it('includes the active node and its direct neighbors', () => {
    const related = relatedHighlight('b', edges);
    expect([...related.nodeIds].sort()).toEqual(['a', 'b', 'c']);
    expect([...related.edgeIds].sort()).toEqual(['e1', 'e2']);
  });

  it('does not mark the hovered node itself as a related neighbor', () => {
    const related = relatedHighlight('b', edges);
    expect(isRelatedNeighbor('b', related.nodeIds, 'b')).toBe(false);
    expect(isRelatedNeighbor('a', related.nodeIds, 'b')).toBe(true);
    expect(isRelatedNeighbor('c', related.nodeIds, 'b')).toBe(true);
  });
});
