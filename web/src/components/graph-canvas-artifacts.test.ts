import { describe, expect, it } from 'vitest';
import type { WorkbenchState } from '../types';
import { outputsByCanvasNode } from './graph-canvas-artifacts';

describe('outputsByCanvasNode', () => {
  it('groups artifacts by originating node and skips duplicate artifact ids', () => {
    const outputs: WorkbenchState['outputs'] = [
      {
        id: 'artifact_1',
        kind: 'video',
        title: 'Vertical teaser',
        nodeId: 'video',
        storageUri: '/api/artifacts/artifact_1/download',
        selected: true,
        meta: '1080 x 1920',
      },
      {
        id: 'artifact_1',
        kind: 'video',
        title: 'Vertical teaser retry',
        nodeId: 'video',
        storageUri: '/api/artifacts/artifact_1/download',
        selected: true,
        meta: 'duplicate retry',
      },
      {
        id: 'artifact_2',
        kind: 'image',
        title: 'Thumbnail',
        nodeId: 'image',
        storageUri: '/api/artifacts/artifact_2/download',
        selected: false,
        meta: '1024 x 1024',
      },
      {
        id: 'artifact_3',
        kind: 'text',
        title: 'Legacy payload without node id',
        storageUri: '/api/artifacts/artifact_3/download',
        selected: false,
        meta: '{}',
      },
    ];

    const grouped = outputsByCanvasNode(outputs);

    expect(grouped.get('video')?.map((output) => output.id)).toEqual(['artifact_1']);
    expect(grouped.get('image')?.map((output) => output.id)).toEqual(['artifact_2']);
    expect(grouped.has('Legacy payload without node id')).toBe(false);
  });
});
