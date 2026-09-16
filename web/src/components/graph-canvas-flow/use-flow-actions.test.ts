import { describe, expect, it } from 'vitest';
import type { WorkbenchState } from '../../types';
import { emptyMediaCardId } from './use-flow-actions';

describe('emptyMediaCardId', () => {
  it('fills only an empty visual media card', () => {
    const nodes = [imageNode('photo'), textNode()];
    expect(emptyMediaCardId('photo', nodes, () => true)).toBe('photo');
    expect(emptyMediaCardId('photo', nodes, () => false)).toBeNull();
    expect(emptyMediaCardId('text', nodes, () => true)).toBeNull();
    expect(emptyMediaCardId('missing', nodes, () => true)).toBeNull();
  });
});

function imageNode(id: string): WorkbenchState['graph']['nodes'][number] {
  return {
    id,
    nodeType: 'input.image',
    title: '图片',
    category: 'Input',
    status: 'queued',
    position: { x: 80, y: 80 },
    provider: null,
    summary: '{}',
  };
}

function textNode(): WorkbenchState['graph']['nodes'][number] {
  return {
    id: 'text',
    nodeType: 'input.text',
    title: '文本',
    category: 'Input',
    status: 'queued',
    position: { x: 40, y: 80 },
    provider: null,
    summary: '{}',
  };
}
