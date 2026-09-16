import { describe, expect, it } from 'vitest';
import { htmlToMarkdown, markdownToHtml } from './text-card-markdown';

describe('text card markdown', () => {
  it('renders headings, emphasis, and lists', () => {
    const html = markdownToHtml(
      ['# Title', '', 'A **bold** and *italic* line', '', '- one', '- two', '', '1. first', '2. second'].join(
        '\n',
      ),
    );
    expect(html).toContain('<h1>Title</h1>');
    expect(html).toContain('<strong>bold</strong>');
    expect(html).toContain('<em>italic</em>');
    expect(html).toContain('<ul><li>one</li><li>two</li></ul>');
    expect(html).toContain('<ol><li>first</li><li>second</li></ol>');
  });

  it('escapes raw HTML in markdown source', () => {
    const html = markdownToHtml('<script>alert(1)</script>');
    expect(html).toContain('&lt;script&gt;');
    expect(html).not.toContain('<script>');
  });

  it('round-trips the supported subset', () => {
    const source = ['# Title', 'Hello **world**', '- alpha', '- beta'].join('\n\n');
    expect(htmlToMarkdown(markdownToHtml(source))).toBe(source);
  });

  it('reads contenteditable headings and lists back to markdown', () => {
    expect(htmlToMarkdown('<h2>Scene</h2><ul><li>wide</li><li>close</li></ul>')).toBe(
      ['## Scene', '- wide\n- close'].join('\n\n'),
    );
  });
});
