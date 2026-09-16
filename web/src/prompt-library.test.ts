import { describe, expect, it } from 'vitest';
import { searchPromptLibrary, type PromptEntry } from './prompt-library';

const items: PromptEntry[] = [
  {
    id: 'a:1',
    title: 'Product hero',
    prompt: 'studio light on a glass bottle',
    tags: ['product'],
    sourceId: 'a',
    sourceName: 'Banana',
  },
  {
    id: 'b:2',
    title: 'Portrait',
    prompt: 'soft window light',
    tags: ['people'],
    sourceId: 'b',
    sourceName: 'YouMind',
  },
];

describe('prompt library search', () => {
  it('filters by title, prompt, tags, and source', () => {
    expect(searchPromptLibrary(items, 'bottle').map((item) => item.id)).toEqual(['a:1']);
    expect(searchPromptLibrary(items, 'people').map((item) => item.id)).toEqual(['b:2']);
    expect(searchPromptLibrary(items, 'youmind').map((item) => item.id)).toEqual(['b:2']);
  });

  it('returns a short first page when the query is empty', () => {
    expect(searchPromptLibrary(items, '  ')).toHaveLength(2);
  });
});
