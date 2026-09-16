const HEADING = /^(#{1,3})\s+(.*)$/;
const UL_ITEM = /^[-*]\s+(.*)$/;
const OL_ITEM = /^\d+[.)]\s+(.*)$/;

export function markdownToHtml(markdown: string): string {
  const source = markdown.replace(/\r\n/g, '\n');
  if (source.trim().length === 0) return '';
  const lines = source.split('\n');
  const html: string[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index] ?? '';
    if (line.trim() === '') {
      index += 1;
      continue;
    }
    const heading = line.match(HEADING);
    if (heading) {
      const level = heading[1].length;
      html.push(`<h${level}>${inlineMarkdown(heading[2])}</h${level}>`);
      index += 1;
      continue;
    }
    const ul = collectList(lines, index, UL_ITEM);
    if (ul) {
      html.push(`<ul>${ul.items.map((item) => `<li>${inlineMarkdown(item)}</li>`).join('')}</ul>`);
      index = ul.next;
      continue;
    }
    const ol = collectList(lines, index, OL_ITEM);
    if (ol) {
      html.push(`<ol>${ol.items.map((item) => `<li>${inlineMarkdown(item)}</li>`).join('')}</ol>`);
      index = ol.next;
      continue;
    }
    const paragraph: string[] = [line];
    index += 1;
    while (index < lines.length) {
      const next = lines[index] ?? '';
      if (
        next.trim() === '' ||
        HEADING.test(next) ||
        UL_ITEM.test(next) ||
        OL_ITEM.test(next)
      ) {
        break;
      }
      paragraph.push(next);
      index += 1;
    }
    html.push(`<p>${paragraph.map((item) => inlineMarkdown(item)).join('<br>')}</p>`);
  }
  return html.join('');
}

export function htmlToMarkdown(html: string): string {
  const withoutComments = html.replace(/<!--[\s\S]*?-->/g, '');
  const blocks = splitBlocks(withoutComments.replace(/\u00a0/g, ' '));
  return blocks
    .map((block) => blockToMarkdown(block))
    .filter((block) => block.length > 0)
    .join('\n\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

function collectList(
  lines: string[],
  start: number,
  pattern: RegExp,
): { items: string[]; next: number } | null {
  if (!pattern.test(lines[start] ?? '')) return null;
  const items: string[] = [];
  let index = start;
  while (index < lines.length) {
    const match = (lines[index] ?? '').match(pattern);
    if (!match) break;
    items.push(match[1] ?? '');
    index += 1;
  }
  return items.length > 0 ? { items, next: index } : null;
}

function inlineMarkdown(text: string): string {
  const escaped = escapeHtml(text);
  return escaped
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/__([^_]+)__/g, '<strong>$1</strong>')
    .replace(/\*([^*]+)\*/g, '<em>$1</em>')
    .replace(/_([^_]+)_/g, '<em>$1</em>');
}

function splitBlocks(html: string): string[] {
  const normalized = html
    .replace(/<\/(div|p|h1|h2|h3|li|ul|ol)>/gi, '</$1>\n')
    .replace(/<br\s*\/?>/gi, '\n');
  return normalized
    .split(/(?=<h[1-3]\b)|(?=<ul\b)|(?=<ol\b)|(?=<p\b)|(?=<div\b)/i)
    .map((block) => block.trim())
    .filter(Boolean);
}

function blockToMarkdown(block: string): string {
  const heading = block.match(/^<h([1-3])\b[^>]*>([\s\S]*?)<\/h\1>$/i);
  if (heading) {
    return `${'#'.repeat(Number(heading[1]))} ${inlineHtmlToMarkdown(heading[2])}`;
  }
  const list = block.match(/^<(ul|ol)\b[^>]*>([\s\S]*?)<\/\1>$/i);
  if (list) {
    const ordered = list[1].toLowerCase() === 'ol';
    const items = [...list[2].matchAll(/<li\b[^>]*>([\s\S]*?)<\/li>/gi)];
    return items
      .map((item, index) => {
        const prefix = ordered ? `${index + 1}. ` : '- ';
        return `${prefix}${inlineHtmlToMarkdown(item[1]).replace(/\n+/g, ' ')}`;
      })
      .join('\n');
  }
  const paragraph = block.match(/^<(p|div)\b[^>]*>([\s\S]*?)<\/\1>$/i);
  if (paragraph) {
    return inlineHtmlToMarkdown(paragraph[2]);
  }
  return inlineHtmlToMarkdown(block);
}

function inlineHtmlToMarkdown(html: string): string {
  return decodeHtml(
    html
      .replace(/<\/?(div|p|span)[^>]*>/gi, '')
      .replace(/<strong\b[^>]*>([\s\S]*?)<\/strong>/gi, '**$1**')
      .replace(/<b\b[^>]*>([\s\S]*?)<\/b>/gi, '**$1**')
      .replace(/<em\b[^>]*>([\s\S]*?)<\/em>/gi, '*$1*')
      .replace(/<i\b[^>]*>([\s\S]*?)<\/i>/gi, '*$1*')
      .replace(/<[^>]+>/g, ''),
  )
    .replace(/\n+/g, '\n')
    .trim();
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function decodeHtml(text: string): string {
  return text
    .replace(/&nbsp;/gi, ' ')
    .replace(/&quot;/gi, '"')
    .replace(/&#39;/gi, "'")
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&amp;/gi, '&');
}
