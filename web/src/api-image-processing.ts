import { z } from 'zod';
import { ImageProcessingJobSchema, type ImageProcessingJob } from './types';

export type ImageProcessingProfile = {
  name: string;
  model: string;
  supportsQuality: boolean;
  supportsSizeTier: boolean;
};

export type ImageProcessingCapabilities = {
  provider: string;
  defaults: Partial<Record<ImageProcessingJob['intent'], string>>;
  profiles: Partial<Record<ImageProcessingJob['intent'], ImageProcessingProfile[]>>;
};

const CapabilitiesSchema = z.object({
  provider: z.string().min(1),
  defaults: z.record(z.string(), z.string().min(1)),
  profiles: z.record(z.string(), z.array(z.object({
    name: z.string().min(1),
    model: z.string().min(1),
    supportsQuality: z.boolean(),
    supportsSizeTier: z.boolean(),
  }))),
});

const ExecutionSchema = z.object({
  job: ImageProcessingJobSchema,
});

export async function fetchImageProcessingCapabilities(
  workspaceId: string,
  signal?: AbortSignal,
): Promise<ImageProcessingCapabilities> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/image-processing-capabilities`,
    { signal },
  );
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) throw responseError(payload, response.status);
  return CapabilitiesSchema.parse(payload) as ImageProcessingCapabilities;
}

export async function createImageProcessingJob(
  workspaceId: string,
  input: {
    sourceNodeId: string;
    intent: ImageProcessingJob['intent'];
    profile?: string;
    parameters: Record<string, unknown>;
    source: Blob;
    filename: string;
  },
  signal?: AbortSignal,
): Promise<z.infer<typeof ExecutionSchema>> {
  const body = new FormData();
  body.append('request', JSON.stringify({
    sourceNodeId: input.sourceNodeId,
    intent: input.intent,
    ...(input.profile ? { profile: input.profile } : {}),
    parameters: input.parameters,
  }));
  body.append('file', input.source, input.filename);
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/image-processing-jobs`,
    { method: 'POST', body, signal },
  );
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) throw responseError(payload, response.status);
  return ExecutionSchema.parse(payload);
}

export async function fetchImageProcessingJob(
  workspaceId: string,
  jobId: string,
  signal?: AbortSignal,
): Promise<ImageProcessingJob> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/image-processing-jobs/${encodeURIComponent(jobId)}`,
    { signal },
  );
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) throw responseError(payload, response.status);
  return ImageProcessingJobSchema.parse(payload);
}

export async function waitForImageProcessingJob(
  workspaceId: string,
  jobId: string,
  signal?: AbortSignal,
): Promise<ImageProcessingJob> {
  for (;;) {
    const job = await fetchImageProcessingJob(workspaceId, jobId, signal);
    if (job.status !== 'queued' && job.status !== 'running') return job;
    await abortableDelay(1_000, signal);
  }
}

export async function linkImageProcessingResult(
  workspaceId: string,
  jobId: string,
  input: { resultNodeId: string; outputUploadId: string },
  signal?: AbortSignal,
): Promise<ImageProcessingJob> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/image-processing-jobs/${encodeURIComponent(jobId)}`,
    {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
      signal,
    },
  );
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) throw responseError(payload, response.status);
  return ImageProcessingJobSchema.parse(payload);
}

function abortableDelay(milliseconds: number, signal?: AbortSignal): Promise<void> {
  if (signal?.aborted) return Promise.reject(signal.reason ?? new DOMException('Aborted', 'AbortError'));
  return new Promise((resolve, reject) => {
    const timer = globalThis.setTimeout(() => {
      signal?.removeEventListener('abort', onAbort);
      resolve();
    }, milliseconds);
    const onAbort = () => {
      globalThis.clearTimeout(timer);
      reject(signal?.reason ?? new DOMException('Aborted', 'AbortError'));
    };
    signal?.addEventListener('abort', onAbort, { once: true });
  });
}

function responseError(payload: unknown, status: number): Error {
  const message = payload && typeof payload === 'object' && 'error' in payload
    && typeof (payload as { error?: unknown }).error === 'string'
    ? (payload as { error: string }).error
    : `image processing request failed: ${status}`;
  return new Error(message);
}
