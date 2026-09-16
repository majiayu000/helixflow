import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  createImageProcessingJob,
  fetchImageProcessingJob,
  fetchImageProcessingCapabilities,
  linkImageProcessingResult,
  waitForImageProcessingJob,
} from './api-image-processing';

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('image processing API', () => {
  it('reads Helixflow capabilities for the current workspace', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({
      provider: 'atlas',
      defaults: { outpaint: 'gpt-image-2' },
      profiles: {
        outpaint: [{
          name: 'gpt-image-2',
          model: 'openai/gpt-image-2/edit',
          supportsQuality: true,
          supportsSizeTier: true,
        }],
      },
    }), { status: 200, headers: { 'content-type': 'application/json' } })));

    await expect(fetchImageProcessingCapabilities('ws 1')).resolves.toMatchObject({
      provider: 'atlas',
      defaults: { outpaint: 'gpt-image-2' },
    });
    expect(fetch).toHaveBeenCalledWith(
      '/api/workspaces/ws%201/image-processing-capabilities',
      { signal: undefined },
    );
  });

  it('submits source and typed intent as one multipart Helixflow request', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(executionResponse()));
    const source = new Blob(['png'], { type: 'image/png' });

    await expect(createImageProcessingJob('ws_1', {
      sourceNodeId: 'photo',
      intent: 'outpaint',
      profile: 'gpt-image-2',
      parameters: { left: 64, top: 64, right: 64, bottom: 64, prompt: 'fill' },
      source,
      filename: 'photo-outpaint.png',
    })).resolves.toMatchObject({
      job: { id: 'imgjob_1', status: 'queued' },
    });

    const init = vi.mocked(fetch).mock.calls[0][1] as RequestInit;
    expect(init.method).toBe('POST');
    expect(init.body).toBeInstanceOf(FormData);
    const body = init.body as FormData;
    expect(JSON.parse(String(body.get('request')))).toEqual({
      sourceNodeId: 'photo',
      intent: 'outpaint',
      profile: 'gpt-image-2',
      parameters: { left: 64, top: 64, right: 64, bottom: 64, prompt: 'fill' },
    });
    expect(body.get('file')).toBeInstanceOf(File);
  });

  it('reads the backend-owned terminal job', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jobResponse('succeeded')));

    await expect(fetchImageProcessingJob('ws_1', 'imgjob_1')).resolves.toMatchObject({
      id: 'imgjob_1',
      status: 'succeeded',
      outputUploadId: 'upload_1',
    });
    expect(fetch).toHaveBeenCalledWith(
      '/api/workspaces/ws_1/image-processing-jobs/imgjob_1',
      { signal: undefined },
    );
  });

  it('polls until the backend reaches a terminal state', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('fetch', vi.fn()
      .mockResolvedValueOnce(jobResponse('running'))
      .mockResolvedValueOnce(jobResponse('succeeded')));

    const pending = waitForImageProcessingJob('ws_1', 'imgjob_1');
    await vi.advanceTimersByTimeAsync(1_000);
    await expect(pending).resolves.toMatchObject({ status: 'succeeded' });
    expect(fetch).toHaveBeenCalledTimes(2);
    vi.useRealTimers();
  });

  it('links the backend output to the result node', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jobResponse('succeeded')));

    await expect(linkImageProcessingResult('ws_1', 'imgjob_1', {
      resultNodeId: 'image_outpaint',
      outputUploadId: 'upload_1',
    })).resolves.toMatchObject({ status: 'succeeded', resultNodeId: 'image_outpaint' });
  });
});

function executionResponse(): Response {
  return new Response(JSON.stringify({
    job: jobValue('queued'),
  }), { status: 202, headers: { 'content-type': 'application/json' } });
}

function jobResponse(status: 'queued' | 'running' | 'succeeded'): Response {
  return new Response(JSON.stringify(jobValue(status)), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

function jobValue(status: 'queued' | 'running' | 'succeeded') {
  return {
    id: 'imgjob_1',
    workspaceId: 'ws_1',
    sourceNodeId: 'photo',
    resultNodeId: status === 'succeeded' ? 'image_outpaint' : null,
    intent: 'outpaint',
    profile: 'gpt-image-2',
    providerTaskId: status === 'queued' ? null : 'provider_task_1',
    provider: status === 'queued' ? null : 'atlas',
    model: status === 'queued' ? null : 'openai/gpt-image-2/edit',
    outputUploadId: status === 'succeeded' ? 'upload_1' : null,
    status,
    error: null,
    createdAt: '2026-09-02 00:00:00',
    updatedAt: '2026-09-02 00:00:01',
    completedAt: status === 'succeeded' ? '2026-09-02 00:00:01' : null,
  };
}
