import { describe, expect, it } from 'vitest';
import { internalNodeBox, mediaCardAnchor } from './card-edge-geometry';

describe('media card edge anchors', () => {
  it('pins to the left and right midpoints of the card box', () => {
    const box = { x: 100, y: 40, width: 280, height: 280 };
    expect(mediaCardAnchor(box, 'right')).toEqual({ x: 380, y: 180 });
    expect(mediaCardAnchor(box, 'left')).toEqual({ x: 100, y: 180 });
  });

  it('does not invent a box when the node has not been measured', () => {
    expect(
      internalNodeBox({
        internals: { positionAbsolute: { x: 20, y: 40 } },
        width: 0,
        height: 0,
      }),
    ).toBeNull();
  });
});
