import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import type { NodeDefinition } from '../../types';
import { CanvasAddMenuPanel } from './canvas-add-menu';

describe('CanvasAddMenuPanel', () => {
  it('offers upload and paste at the click position', () => {
    const markup = renderToStaticMarkup(
      <CanvasAddMenuPanel
        definitions={new Map([
          ['input.text', definition('input.text', '文本')],
          ['input.image', definition('input.image', '图片')],
          ['input.video', definition('input.video', '视频')],
          ['input.audio', definition('input.audio', '音频')],
        ])}
        menu={{ clientX: 40, clientY: 80, flow: { x: 120, y: 160 } }}
        onAdd={vi.fn()}
        onClose={vi.fn()}
        onPaste={vi.fn()}
        onUpload={vi.fn()}
        view={{ x: 0, y: 0, z: 1 }}
      />,
    );

    expect(markup).toContain('添加节点');
    expect(markup).toContain('上传');
    expect(markup).toContain('粘贴');
    expect(markup).toContain('⌘V');
    expect(markup).toContain('type="file"');
    expect(markup).toContain('left:120px');
    expect(markup).toContain('top:160px');
  });
});

function definition(type: string, title: string): NodeDefinition {
  return { type, title, category: 'Input' } as NodeDefinition;
}
