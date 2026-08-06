import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  DEFAULT_GRAPH_VIEW,
  loadGraphCanvasView,
  saveGraphCanvasView,
} from './graph-canvas-navigation';

describe('graph canvas view persistence', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('treats a localStorage object without the Storage API as unavailable', () => {
    vi.stubGlobal('localStorage', {});

    expect(loadGraphCanvasView('workspace-1')).toEqual(DEFAULT_GRAPH_VIEW);
    expect(() => saveGraphCanvasView('workspace-1', { x: 1, y: 2, z: 1 })).not.toThrow();
  });

  it('treats localStorage quota failures as best-effort persistence', () => {
    vi.stubGlobal('localStorage', {
      getItem: vi.fn(() => null),
      setItem: vi.fn(() => {
        throw new DOMException('quota exceeded', 'QuotaExceededError');
      }),
    });

    expect(() => saveGraphCanvasView('workspace-1', { x: 1, y: 2, z: 1 })).not.toThrow();
  });
});
