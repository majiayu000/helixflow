import { describe, expect, it } from 'vitest';
import { canvasCapabilities } from './graph-canvas-capabilities';

describe('graph canvas capabilities', () => {
  it('keeps view mode select/pan/zoom only', () => {
    expect(canvasCapabilities('view', true)).toEqual({
      select: true,
      pan: true,
      zoom: true,
      move: false,
      resize: false,
      connect: false,
      delete: false,
      paste: false,
    });
  });

  it('enables mutation capabilities only in edit mode with a handler', () => {
    expect(canvasCapabilities('edit', true)).toMatchObject({
      move: true,
      resize: true,
      connect: true,
      delete: true,
      paste: true,
    });
    expect(canvasCapabilities('edit', false)).toMatchObject({
      move: false,
      resize: false,
      connect: false,
      delete: false,
      paste: false,
    });
  });

  it('keeps review mode fail closed for mutations', () => {
    expect(canvasCapabilities('review', true)).toMatchObject({
      move: false,
      resize: false,
      connect: false,
      delete: false,
      paste: false,
    });
  });
});
