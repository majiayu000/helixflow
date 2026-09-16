export type PromptSource = {
  id: string;
  name: string;
  url: string;
  enabled: boolean;
};

export type PromptEntry = {
  id: string;
  title: string;
  prompt: string;
  tags: string[];
  sourceId: string;
  sourceName: string;
};

const SOURCE_KEY = 'helixflow:prompt-sources:v1';
const CACHE_KEY = 'helixflow:prompt-cache:v1';
const CACHE_TTL_MS = 60 * 60 * 1000;

export const DEFAULT_PROMPT_SOURCES: PromptSource[] = [
  source('banana-prompt-quicker', 'Banana Prompt Quicker'),
  source('davidwu-gpt-image2-prompts', 'DavidWu GPT Image 2'),
  source('freestylefly-gpt-image-2', 'Freestylefly GPT Image 2'),
  source('awesome-gpt-image', 'Awesome GPT Image'),
  source('awesome-gpt4o-image-prompts', 'Awesome GPT-4o'),
  source('youmind-gpt-image-2', 'YouMind GPT Image 2'),
  source('youmind-nano-banana-pro', 'YouMind Nano Banana Pro'),
];

export function loadPromptSources(): PromptSource[] {
  const stored = readJson(SOURCE_KEY);
  if (!Array.isArray(stored) || stored.length === 0) return DEFAULT_PROMPT_SOURCES;
  return stored.flatMap((item) => {
    if (!item || typeof item !== 'object') return [];
    const record = item as Record<string, unknown>;
    if (typeof record.id !== 'string' || typeof record.url !== 'string') return [];
    return [{
      id: record.id,
      name: typeof record.name === 'string' ? record.name : record.id,
      url: record.url,
      enabled: record.enabled !== false,
    }];
  });
}

export function savePromptSources(sources: PromptSource[]): void {
  writeJson(SOURCE_KEY, sources);
}

export async function loadPromptLibrary(force = false): Promise<PromptEntry[]> {
  const cached = readJson(CACHE_KEY) as { fetchedAt?: number; items?: PromptEntry[] } | null;
  if (!force && cached?.items && typeof cached.fetchedAt === 'number' && Date.now() - cached.fetchedAt < CACHE_TTL_MS) {
    return cached.items;
  }
  const sources = loadPromptSources().filter((item) => item.enabled);
  const results = await Promise.allSettled(sources.map((item) => fetchPromptSource(item)));
  const items = results.flatMap((result, index) => (
    result.status === 'fulfilled' ? result.value : cachedItemsFor(cached?.items, sources[index]?.id)
  ));
  if (items.length > 0) writeJson(CACHE_KEY, { fetchedAt: Date.now(), items });
  else if (cached?.items) return cached.items;
  return items;
}

export function searchPromptLibrary(items: PromptEntry[], query: string): PromptEntry[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return items.slice(0, 80);
  return items.filter((item) => (
    `${item.title} ${item.prompt} ${item.tags.join(' ')} ${item.sourceName}`.toLowerCase().includes(needle)
  )).slice(0, 80);
}

async function fetchPromptSource(source: PromptSource): Promise<PromptEntry[]> {
  const response = await fetch(source.url);
  if (!response.ok) throw new Error(`${source.name} 刷新失败`);
  const payload = await response.json();
  const rows = Array.isArray(payload) ? payload : Array.isArray(payload.items) ? payload.items : [];
  return rows.flatMap((row: unknown, index: number) => {
    if (!row || typeof row !== 'object') return [];
    const record = row as Record<string, unknown>;
    const prompt = typeof record.prompt === 'string'
      ? record.prompt
      : typeof record.text === 'string'
        ? record.text
        : '';
    if (!prompt.trim()) return [];
    const title = typeof record.title === 'string' && record.title.trim()
      ? record.title
      : prompt.slice(0, 36);
    const tags = Array.isArray(record.tags)
      ? record.tags.filter((tag): tag is string => typeof tag === 'string')
      : [];
    return [{
      id: `${source.id}:${typeof record.id === 'string' ? record.id : index}`,
      title,
      prompt,
      tags,
      sourceId: source.id,
      sourceName: source.name,
    }];
  });
}

function cachedItemsFor(items: PromptEntry[] | undefined, sourceId?: string): PromptEntry[] {
  if (!items || !sourceId) return [];
  return items.filter((item) => item.sourceId === sourceId);
}

function source(id: string, name: string): PromptSource {
  return {
    id,
    name,
    url: `https://raw.githubusercontent.com/yukkcat/image-prompts/main/dist/sources/${id}.json`,
    enabled: true,
  };
}

function readJson(key: string): unknown {
  try {
    return JSON.parse(globalThis.localStorage?.getItem(key) ?? 'null');
  } catch {
    return null;
  }
}

function writeJson(key: string, value: unknown): void {
  try {
    globalThis.localStorage?.setItem(key, JSON.stringify(value));
  } catch {
    // Best-effort cache.
  }
}
