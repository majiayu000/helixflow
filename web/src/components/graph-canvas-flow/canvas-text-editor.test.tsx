import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { TextCardEditor } from './canvas-image-modes';
import type { WorkbenchState } from '../../types';

describe('TextCardEditor', () => {
  it('shows the format toolbar and double-click placeholder', () => {
    const markup = renderToStaticMarkup(
      <TextCardEditor
        node={textNode()}
        onSave={() => undefined}
        value=""
        view={{ x: 0, y: 0, z: 1 }}
      />,
    );
    expect(markup).toContain('canvas-text-toolbar');
    expect(markup).toContain('>H1<');
    expect(markup).toContain('>H2<');
    expect(markup).toContain('>H3<');
    expect(markup).toContain('contentEditable="true"');
    expect(markup).toContain('双击开始编辑...');
    expect(markup).not.toContain('textarea');
  });
});

function textNode(): WorkbenchState['graph']['nodes'][number] {
  return {
    id: 'input_text',
    nodeType: 'input.text',
    title: 'Text',
    category: 'Input',
    status: 'queued',
    position: { x: 40, y: 80 },
    size: { width: 320, height: 240 },
    provider: null,
    summary: '{}',
  };
}
