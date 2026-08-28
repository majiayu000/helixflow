import { createElement } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  DEFAULT_GRAPH_VIEW,
  loadGraphCanvasView,
  saveGraphCanvasView,
} from './graph-canvas-navigation';
import {
  shouldRefreshViewportSlice,
  useFlowViewport,
} from './graph-canvas-flow/use-viewport';
import { edgesForViewport, nodeIdsForViewport } from './graph-canvas-rendering';
import type { GraphNodeState, WorkbenchState } from '../types';

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

describe('dense graph viewport slice refresh', () => {
  it('does not commit transient React Flow movement to the domain view', async () => {
    let renderer: ReactTestRenderer | null = null;
    let latest: ReturnType<typeof useFlowViewport> | null = null;
    const Harness = () => {
      latest = useFlowViewport('workspace-1');
      return null;
    };
    const result = () => {
      if (!latest) throw new Error('viewport hook did not render');
      return latest;
    };

    await act(async () => {
      renderer = create(createElement(Harness));
    });
    const initial = result().view;
    await act(async () => {
      result().onMove(null, { x: 180, y: -120, zoom: 1.1 });
    });

    expect(result().view).toEqual(initial);

    await act(async () => {
      result().onMoveEnd(null, { x: 180, y: -120, zoom: 1.1 });
    });
    expect(result().view).toEqual({ x: 180, y: -120, z: 1.1 });
    await act(async () => renderer?.unmount());
  });

  it('keeps the current slice during small pan and zoom movements', () => {
    expect(
      shouldRefreshViewportSlice(
        { x: 0, y: 0, z: 1 },
        { x: 180, y: -180, z: 1.1 },
      ),
    ).toBe(false);
  });

  it('refreshes after crossing the overscan movement threshold', () => {
    expect(
      shouldRefreshViewportSlice(
        { x: 0, y: 0, z: 1 },
        { x: 241, y: 0, z: 1 },
      ),
    ).toBe(true);
  });

  it('refreshes after a material zoom change', () => {
    expect(
      shouldRefreshViewportSlice(
        { x: 0, y: 0, z: 1 },
        { x: 0, y: 0, z: 1.21 },
      ),
    ).toBe(true);
  });
});

describe('dense graph edge rendering', () => {
  it('keeps only edges whose target node is near the viewport above the density limit', () => {
    const edges = [edge('source', 'near'), edge('source', 'far')];

    expect(
      edgesForViewport(
        [node('source', 0, 0), node('near', 400, 100), node('far', 12_000, 8_000)],
        edges,
        { x: 0, y: 0, z: 1 },
        { width: 1200, height: 800 },
        1,
      ),
    ).toEqual([edges[0]]);
  });

  it('returns the complete edge array unchanged below the density limit', () => {
    const edges = [edge('source', 'near'), edge('source', 'far')];

    expect(
      edgesForViewport(
        [node('source', 0, 0), node('near', 400, 100), node('far', 12_000, 8_000)],
        edges,
        { x: 0, y: 0, z: 1 },
        { width: 1200, height: 800 },
        edges.length,
      ),
    ).toBe(edges);
  });
});

describe('dense graph node rendering', () => {
  it('keeps viewport nodes and the endpoints required by visible edges', () => {
    const nodes = [
      node('source', 12_000, 8_000),
      node('near', 400, 100),
      node('far', 14_000, 8_000),
    ];
    const visibleEdges = [edge('source', 'near')];

    expect(
      nodeIdsForViewport(
        nodes,
        visibleEdges,
        { x: 0, y: 0, z: 1 },
        { width: 1200, height: 800 },
        1,
      ),
    ).toEqual(new Set(['source', 'near']));
  });

  it('returns null below the density limit so callers preserve the complete array', () => {
    expect(
      nodeIdsForViewport(
        [node('near', 400, 100)],
        [],
        { x: 0, y: 0, z: 1 },
        { width: 1200, height: 800 },
        1,
      ),
    ).toBeNull();
  });
});

function node(id: string, x: number, y: number): GraphNodeState {
  return {
    cached: false,
    category: 'Text',
    id,
    nodeType: 'input.text',
    position: { x, y },
    provider: null,
    status: 'queued',
    summary: 'input.text',
    title: id,
  };
}

function edge(
  source: string,
  target: string,
): WorkbenchState['graph']['edges'][number] {
  return {
    from: { nodeId: source, port: 'text' },
    id: `${source}-${target}`,
    kind: 'text',
    to: { nodeId: target, port: 'text' },
  };
}
