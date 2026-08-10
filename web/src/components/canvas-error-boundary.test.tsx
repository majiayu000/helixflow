import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CanvasErrorBoundary } from './canvas-error-boundary';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe('CanvasErrorBoundary', () => {
  let renderer: ReactTestRenderer | null = null;

  beforeEach(() => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
  });

  afterEach(async () => {
    if (renderer) await act(async () => renderer?.unmount());
    renderer = null;
    vi.restoreAllMocks();
  });

  function root() {
    if (!renderer) throw new Error('renderer is not mounted');
    return renderer.root;
  }

  it('contains a canvas render failure and retries without losing the workbench', async () => {
    let shouldThrow = true;
    function CanvasChild() {
      if (shouldThrow) throw new Error('canvas render failed');
      return <div>canvas restored</div>;
    }

    await act(async () => {
      renderer = create(
        <CanvasErrorBoundary resetKey="ws_1">
          <CanvasChild />
        </CanvasErrorBoundary>,
      );
    });

    expect(root().findByProps({ role: 'alert' })).toBeDefined();
    expect(root().findByType('h2').children.join('')).toBe('画布暂时无法显示');

    shouldThrow = false;
    await act(async () => root().findByType('button').props.onClick());

    expect(root().findByType('div').children).toContain('canvas restored');
  });

  it('resets the failed canvas when the workspace changes', async () => {
    function BrokenCanvas(): ReactNode {
      throw new Error('broken canvas');
    }

    await act(async () => {
      renderer = create(
        <CanvasErrorBoundary resetKey="ws_1">
          <BrokenCanvas />
        </CanvasErrorBoundary>,
      );
    });
    expect(root().findByProps({ role: 'alert' })).toBeDefined();

    await act(async () => {
      renderer?.update(
        <CanvasErrorBoundary resetKey="ws_2">
          <div>next workspace</div>
        </CanvasErrorBoundary>,
      );
    });

    expect(root().findByType('div').children).toContain('next workspace');
  });
});
