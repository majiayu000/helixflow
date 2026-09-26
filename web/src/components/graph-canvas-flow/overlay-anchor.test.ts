import { describe, expect, it } from 'vitest';
import type { WorkbenchState } from '../../types';
import { flowAboveCenterStyle, flowBelowStyle, flowCoverStyle } from './overlay-anchor';

describe('overlay-anchor', () => {
  it('places chrome in flow space so ViewportPortal can follow the camera', () => {
    const node = {
      id: 'card',
      nodeType: 'input.image',
      title: 'Image',
      category: 'Input',
      status: 'queued',
      position: { x: 80, y: 40 },
      size: { width: 280, height: 280 },
      provider: null,
      summary: '',
    } as WorkbenchState['graph']['nodes'][number];

    expect(flowCoverStyle(node)).toEqual({ left: 80, top: 40, width: 280, height: 280 });
    expect(flowAboveCenterStyle(node, 10)).toEqual({ left: 220, top: 30 });
    expect(flowBelowStyle(node, 14)).toEqual({ left: 80, top: 334, width: 280 });
  });
});
