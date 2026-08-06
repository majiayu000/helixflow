import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { DirtyNavigationDialog, nextDialogFocusIndex } from './dirty-navigation-dialog';

describe('dirty navigation dialog', () => {
  it('renders the shared commit, discard, and cancel decisions', () => {
    const markup = renderToStaticMarkup(
      <DirtyNavigationDialog
        busy={false}
        target={{ kind: 'workspace', workspaceId: 'ws_b' }}
        onDecision={vi.fn()}
      />,
    );

    expect(markup).toContain('处理未提交编辑');
    expect(markup).toContain('workspace ws_b');
    expect(markup).toContain('取消');
    expect(markup).toContain('放弃并继续');
    expect(markup).toContain('提交并继续');
  });

  it('renders nothing without a pending navigation', () => {
    expect(renderToStaticMarkup(
      <DirtyNavigationDialog busy={false} target={null} onDecision={vi.fn()} />,
    )).toBe('');
  });

  it('wraps keyboard focus only at dialog boundaries', () => {
    expect(nextDialogFocusIndex(2, 3, false)).toBe(0);
    expect(nextDialogFocusIndex(0, 3, true)).toBe(2);
    expect(nextDialogFocusIndex(1, 3, false)).toBeNull();
    expect(nextDialogFocusIndex(-1, 3, false)).toBe(0);
  });
});
