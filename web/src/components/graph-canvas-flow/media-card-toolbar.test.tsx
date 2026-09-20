import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import type { WorkbenchState } from '../../types';
import { MediaCardToolbar } from './media-card-toolbar';

describe('MediaCardToolbar', () => {
  it('gives video and audio cards replace, copy, download, and delete', () => {
    const markup = renderToStaticMarkup(
      <MediaCardToolbar
        canDownload
        node={videoNode()}
        onDelete={vi.fn()}
        onDownload={vi.fn()}
        onDuplicate={vi.fn()}
        onReplace={vi.fn()}
        onSaveAsset={vi.fn()}
        view={{ x: 0, y: 0, z: 1 }}
      />,
    );

    expect(markup).toContain('替换');
    expect(markup).toContain('复制');
    expect(markup).toContain('下载');
    expect(markup).toContain('删除');
    expect(markup).toContain('入库');
  });
});

function videoNode(): WorkbenchState['graph']['nodes'][number] {
  return {
    id: 'input_video',
    nodeType: 'input.video',
    title: 'Video Input',
    category: 'Input',
    status: 'queued',
    position: { x: 80, y: 80 },
    provider: null,
    summary: '',
  };
}
