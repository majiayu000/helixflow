import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import type { WorkbenchState } from '../../types';
import { EmptyCardUpload, mentionItemsFromNodes } from './media-card-overlays';

describe('EmptyCardUpload', () => {
  it('opens a file picker from the overlay button, not the card', () => {
    const markup = renderToStaticMarkup(
      <EmptyCardUpload
        node={emptyImage()}
        onUpload={() => undefined}
        view={{ x: 0, y: 0, z: 1 }}
      />,
    );

    expect(markup).toContain('上传');
    expect(markup).toContain('type="file"');
    expect(markup).toContain('accept="image/*"');
    expect(markup).not.toContain('点击或拖入文件');
  });

  it('mentions other visual cards, not the current one', () => {
    expect(mentionItemsFromNodes([
      emptyImage(),
      { ...emptyImage(), id: 'ref', title: '参考图' },
    ], 'input_image')).toEqual([
      { id: 'ref', label: '参考图', title: '参考图', kind: '图片' },
    ]);
  });
});

function emptyImage(): WorkbenchState['graph']['nodes'][number] {
  return {
    id: 'input_image',
    nodeType: 'input.image',
    title: 'Image Input',
    category: 'Input',
    status: 'queued',
    position: { x: 80, y: 80 },
    provider: null,
    summary: '{}',
  };
}
