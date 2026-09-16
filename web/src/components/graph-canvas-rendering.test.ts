import { describe, expect, it } from 'vitest';

import { applyImageProcessingNodeStates } from './graph-canvas-rendering';
import type { ImageProcessingJob } from '../types';

describe('image processing node state', () => {
  it('shows active and failed jobs on their source nodes and success on the result', () => {
    const states = applyImageProcessingNodeStates(new Map(), [
      job({ id: 'new', sourceNodeId: 'active', status: 'running' }),
      job({ id: 'failed', sourceNodeId: 'broken', status: 'failed' }),
      job({ id: 'done', sourceNodeId: 'source', resultNodeId: 'result', status: 'succeeded' }),
    ]);

    expect(states.get('active')).toBe('running');
    expect(states.get('broken')).toBe('failed');
    expect(states.get('result')).toBe('succeeded');
    expect(states.has('source')).toBe(false);
  });

  it('uses the newest source job when older records follow it', () => {
    const states = applyImageProcessingNodeStates(new Map(), [
      job({ id: 'new', sourceNodeId: 'photo', status: 'running' }),
      job({ id: 'old', sourceNodeId: 'photo', status: 'failed' }),
    ]);

    expect(states.get('photo')).toBe('running');
  });
});

function job(overrides: Partial<ImageProcessingJob>): ImageProcessingJob {
  return {
    id: 'imgjob_1',
    workspaceId: 'ws_1',
    sourceNodeId: 'photo',
    resultNodeId: null,
    intent: 'outpaint',
    profile: 'gpt-image-2',
    providerTaskId: null,
    provider: null,
    model: null,
    outputUploadId: null,
    status: 'queued',
    error: null,
    createdAt: '2026-09-02 00:00:00',
    updatedAt: '2026-09-02 00:00:00',
    completedAt: null,
    ...overrides,
  };
}
