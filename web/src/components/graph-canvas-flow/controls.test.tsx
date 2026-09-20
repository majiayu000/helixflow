import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { CanvasViewControls } from './controls';

describe('CanvasViewControls', () => {
  it('exposes hide-edges, snap, and shortcut help like Tapnow', () => {
    const markup = renderToStaticMarkup(
      <CanvasViewControls
        hideEdges
        instance={null}
        nodes={[]}
        onHelp={vi.fn()}
        onToggleHideEdges={vi.fn()}
        onToggleSnap={vi.fn()}
        snapToGrid={false}
        view={{ x: 0, y: 0, z: 1 }}
        viewportSize={{ width: 900, height: 640 }}
      />,
    );

    expect(markup).toContain('隐藏连线');
    expect(markup).toContain('网格吸附');
    expect(markup).toContain('快捷键说明');
    expect(markup).toContain('aria-pressed="true"');
  });
});
