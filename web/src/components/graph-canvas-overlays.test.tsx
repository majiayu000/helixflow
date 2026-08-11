import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { EmptyCanvas } from './graph-canvas-overlays';

describe('EmptyCanvas', () => {
  it('presents an agent-first composer with executable starter tasks', () => {
    const markup = renderToStaticMarkup(<EmptyCanvas onPrompt={() => undefined} />);

    expect(markup).toContain('AGENT CANVAS');
    expect(markup).toContain('描述结果，工作流随后出现');
    expect(markup).toContain('aria-label="描述要生成的结果"');
    expect(markup).toContain('开始创建');
    expect(markup).toContain('示例任务');
  });
});
