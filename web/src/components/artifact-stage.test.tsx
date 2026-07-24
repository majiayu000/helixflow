import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { fetchArtifactText } from '../api';
import type { WorkbenchState } from '../types';
import { ArtifactStage } from './artifact-stage';

const textOutputs = [
  {
    id: 'art_text_1',
    kind: 'text',
    title: 'Prompt draft',
    storageUri: 'artifacts/run_1/step_1-writer.txt',
    selected: true,
    meta: 'text/plain',
    mime: 'text/plain',
    preview: {
      kind: 'text' as const,
      content: 'Artifact: writer\nKind: text',
    },
  },
] as WorkbenchState['outputs'];

describe('ArtifactStage text preview (HF-023)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('no longer renders the metadata-only preview text', () => {
    const markup = renderToStaticMarkup(<ArtifactStage outputs={textOutputs} />);

    expect(markup).not.toContain('Artifact: writer');
    expect(markup).toContain('Loading artifact content');
  });

  it('fetchArtifactText loads real bytes from the content API', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        expect(String(input)).toBe('/api/artifacts/art_text_1/content');
        return new Response('REAL WIRED PROMPT CONTENT', { status: 200 });
      }),
    );

    await expect(fetchArtifactText('art_text_1')).resolves.toBe('REAL WIRED PROMPT CONTENT');
  });

  it('fetchArtifactText surfaces HTTP failures instead of metadata fallback', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('missing', { status: 404 })),
    );

    await expect(fetchArtifactText('art_text_1')).rejects.toThrow(
      'artifact content request failed: 404',
    );
  });
});
