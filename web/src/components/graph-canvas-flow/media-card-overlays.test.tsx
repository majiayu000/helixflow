import { renderToStaticMarkup } from 'react-dom/server';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { describe, expect, it } from 'vitest';
import type { WorkbenchState } from '../../types';
import { EmptyCardUpload, MediaCardComposer, mentionItemsFromNodes } from './media-card-overlays';

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

  it('lets a text card choose image or video, duration, and count', async () => {
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <MediaCardComposer
          mentionItems={[]}
          modelCatalog={null}
          node={textNode()}
          view={{ x: 0, y: 0, z: 1 }}
          workflowPrompt="hero shot"
        />,
      );
    });
    const html = JSON.stringify(renderer!.toJSON());
    expect(html).toContain('出图');
    expect(html).toContain('出视频');
    expect(html).toContain('x1');
    expect(html).not.toContain('4s');

    const kind = renderer!.root.findByProps({ 'aria-label': '生成类型' });
    await act(async () => kind.props.onChange({ currentTarget: { value: 'video' } }));
    expect(JSON.stringify(renderer!.toJSON())).toContain('4s');
    await act(async () => renderer!.unmount());
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

function textNode(): WorkbenchState['graph']['nodes'][number] {
  return {
    ...emptyImage(),
    id: 'copy',
    nodeType: 'input.text',
    title: '文案',
  };
}
