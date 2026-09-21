import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  DEFAULT_IMAGE_EDIT_PROMPTS,
  defaultImageCanvasToolRequest,
  imageCanvasPrepFilename,
  imageCanvasResultFilename,
  imageCanvasToolLabel,
} from './image-canvas-tools';
import { buildImageCanvasToolResultProposalInput } from './components/graph-canvas-editing';
import { useModelPreferenceStore } from './model-picker';
import { createGenerateFromMediaCardAction, createImageCanvasToolAction } from './store-image-edit';
import {
  createImageProcessingJob,
  linkImageProcessingResult,
  waitForImageProcessingJob,
} from './api-image-processing';
import type { ImageProcessingJob } from './types';

vi.mock('./api-image-processing', () => ({
  createImageProcessingJob: vi.fn(),
  linkImageProcessingResult: vi.fn(),
  waitForImageProcessingJob: vi.fn(),
}));

afterEach(() => {
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

describe('image canvas generative tools', () => {
  it('keeps Chinese UI labels and English model prompts separate', () => {
    expect(imageCanvasToolLabel('outpaint')).toBe('扩图');
    expect(imageCanvasToolLabel('inpaint')).toBe('擦除');
    expect(imageCanvasToolLabel('cutout')).toBe('抠图');
    expect(imageCanvasToolLabel('upscale')).toBe('超分');
    expect(imageCanvasToolLabel('enhance')).toBe('增强');
    expect(DEFAULT_IMAGE_EDIT_PROMPTS.inpaint).toMatch(/transparent region/);
    expect(DEFAULT_IMAGE_EDIT_PROMPTS.redraw).toMatch(/masked region/);
  });

  it('names prepared canvases without overwriting the source file', () => {
    expect(imageCanvasPrepFilename('产品主图', 'outpaint')).toBe('产品主图-outpaint-canvas.png');
    expect(imageCanvasPrepFilename('产品主图', 'inpaint')).toBe('产品主图-inpaint-canvas.png');
    expect(imageCanvasResultFilename('产品主图', 'outpaint')).toBe('产品主图-outpaint.png');
    expect(imageCanvasResultFilename('产品主图', 'cutout')).toBe('产品主图-cutout.png');
  });

  it('persists a completed processor result as a connected image card', () => {
    const input = buildImageCanvasToolResultProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: ['photo'],
      sourceNodeId: 'photo',
      sourceTitle: '产品主图',
      sourceX: 40,
      sourceY: 50,
      sourceWidth: 240,
      kind: 'outpaint',
      storageUri: 'upload://result',
      size: { width: 1024, height: 768 },
    });
    expect(input.ops).toEqual([
      {
        op: 'spawn_node',
        id: 'image_outpaint',
        node_type: 'input.image',
        title: '产品主图 扩图',
        params: { storage_uri: 'upload://result' },
        pos: [304, 50],
        from: 'photo',
      },
      { op: 'resize_node', id: 'image_outpaint', size: [1024, 768] },
    ]);
  });

  it('submits outpaint to Helixflow and writes the completed result', async () => {
    mockExecution('outpaint', 'gpt-image-2', 'openai/gpt-image-2/edit', 1200, 800);
    const appendManualEdit = vi.fn().mockResolvedValue(undefined);
    const queueRun = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('fetch', vi.fn().mockImplementation(() => Promise.resolve(
      new Response(new Blob(['source'], { type: 'image/png' })),
    )));
    const state = {
      workspace: { id: 'ws_1', versionId: 'ver_1' },
      graph: {
        nodes: [{
          id: 'photo',
          nodeType: 'input.image',
          title: 'Photo',
          position: { x: 10, y: 20 },
        }],
      },
      workflowGraph: {
        nodes: { photo: { params: { storage_uri: 'upload://photo' } } },
      },
      outputs: [],
    };
    const action = createImageCanvasToolAction(
      vi.fn() as never,
      (() => ({ state, editSession: null, appendManualEdit, queueRun })) as never,
      {
        currentGeneration: () => 1,
        signal: () => undefined,
        isActive: () => true,
      } as never,
    );

    await action('photo', {
      kind: 'outpaint',
      left: 64,
      top: 64,
      right: 64,
      bottom: 64,
      prompt: DEFAULT_IMAGE_EDIT_PROMPTS.outpaint,
      profile: 'gpt-image-2',
      quality: 'medium',
      sizeTier: '2k',
    });

    expect(createImageProcessingJob).toHaveBeenCalledWith('ws_1', expect.objectContaining({
      sourceNodeId: 'photo',
      intent: 'outpaint',
      profile: 'gpt-image-2',
      filename: 'Photo-outpaint-canvas.png',
      parameters: expect.objectContaining({
        left: 64,
        top: 64,
        right: 64,
        bottom: 64,
        quality: 'medium',
        sizeTier: '2k',
      }),
    }), undefined);
    expect(appendManualEdit).toHaveBeenCalledOnce();
    expect(appendManualEdit).toHaveBeenCalledWith(expect.objectContaining({
      ops: expect.arrayContaining([
        expect.objectContaining({
          op: 'spawn_node',
          node_type: 'input.image',
          params: { storage_uri: 'upload://upload_result' },
          from: 'photo',
        }),
      ]),
    }));
    expect(queueRun).not.toHaveBeenCalled();
    expect(linkImageProcessingResult).toHaveBeenNthCalledWith(
      1,
      'ws_1',
      'imgjob_1',
      {
        resultNodeId: 'image_outpaint',
        outputUploadId: 'upload_result',
      },
      undefined,
    );
  });

  it('submits cutout as a prompt-free Helixflow intent', async () => {
    mockExecution('cutout', 'youchuan-v8.2', 'youchuan/v8.2/remove-background', 600, 600);
    const appendManualEdit = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('fetch', vi.fn().mockImplementation(() => Promise.resolve(
      new Response(new Blob(['source'], { type: 'image/png' })),
    )));
    const state = {
      workspace: { id: 'ws_1', versionId: 'ver_1' },
      graph: { nodes: [{ id: 'photo', nodeType: 'input.image', title: 'Photo', position: { x: 0, y: 0 } }] },
      workflowGraph: { nodes: { photo: { params: { storage_uri: 'upload://photo' } } } },
      outputs: [],
    };
    const action = createImageCanvasToolAction(
      vi.fn() as never,
      (() => ({ state, editSession: null, appendManualEdit })) as never,
      { currentGeneration: () => 1, signal: () => undefined, isActive: () => true } as never,
    );

    await action('photo', defaultImageCanvasToolRequest('cutout'));

    expect(createImageProcessingJob).toHaveBeenCalledWith('ws_1', expect.objectContaining({
      intent: 'cutout',
      parameters: {},
    }), undefined);
  });

  it('surfaces a backend-owned failed task without writing a fake client state', async () => {
    vi.mocked(createImageProcessingJob).mockResolvedValue({
      job: imageJob({ intent: 'outpaint', status: 'queued' }),
    });
    vi.mocked(waitForImageProcessingJob).mockResolvedValue(imageJob({
      intent: 'outpaint',
      status: 'failed',
      error: 'provider rejected the image',
    }));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      new Response(new Blob(['source'], { type: 'image/png' })),
    ));
    const state = {
      workspace: { id: 'ws_1', versionId: 'ver_1' },
      graph: {
        nodes: [{
          id: 'photo', nodeType: 'input.image', title: 'Photo', position: { x: 0, y: 0 },
        }],
      },
      workflowGraph: { nodes: { photo: { params: { storage_uri: 'upload://photo' } } } },
      outputs: [],
    };
    const action = createImageCanvasToolAction(
      vi.fn() as never,
      (() => ({ state, editSession: null, appendManualEdit: vi.fn() })) as never,
      { currentGeneration: () => 1, signal: () => undefined, isActive: () => true } as never,
    );

    await expect(action('photo', {
      kind: 'outpaint',
      left: 64,
      top: 64,
      right: 64,
      bottom: 64,
      prompt: DEFAULT_IMAGE_EDIT_PROMPTS.outpaint,
      profile: 'gpt-image-2',
    })).rejects.toThrow('provider rejected the image');

    expect(linkImageProcessingResult).not.toHaveBeenCalled();
  });
});

describe('generate from media card', () => {
  beforeEach(() => {
    useModelPreferenceStore.getState().setPreferredModelId(null);
  });

  it('spawns a connected generate card from an empty image card', async () => {
    const appendManualEdit = vi.fn().mockResolvedValue(undefined);
    const queueRun = vi.fn().mockResolvedValue(undefined);
    const action = createGenerateFromMediaCardAction(
      vi.fn() as never,
      (() => ({
        state: {
          workspace: { id: 'ws_1', versionId: 'ver_1' },
          graph: {
            nodes: [{
              id: 'photo',
              nodeType: 'input.image',
              title: '图片',
              position: { x: 80, y: 40 },
              size: { width: 320, height: 240 },
            }],
            edges: [],
          },
          workflowGraph: { nodes: { photo: { params: {} } } },
          outputs: [],
        },
        editSession: null,
        appendManualEdit,
        queueRun,
      })) as never,
      { currentGeneration: () => 1, signal: () => undefined, isActive: () => true } as never,
    );

    await action('photo', 'product hero', '16:9');

    expect(appendManualEdit).toHaveBeenCalledWith(expect.objectContaining({
      ops: expect.arrayContaining([
        expect.objectContaining({
          op: 'spawn_node',
          id: 'image_generate',
          node_type: 'image.generate',
          from: 'photo',
        }),
      ]),
    }));
    expect(queueRun).toHaveBeenCalledOnce();
  });

  it('spawns a connected edit card from a generated image instead of rewriting it', async () => {
    const appendManualEdit = vi.fn().mockResolvedValue(undefined);
    const queueRun = vi.fn().mockResolvedValue(undefined);
    const action = createGenerateFromMediaCardAction(
      vi.fn() as never,
      (() => ({
        state: {
          workspace: { id: 'ws_1', versionId: 'ver_1' },
          graph: {
            nodes: [{
              id: 'hero',
              nodeType: 'image.generate',
              title: '主视觉',
              position: { x: 80, y: 40 },
              size: { width: 280, height: 280 },
            }],
            edges: [],
          },
          workflowGraph: {
            nodes: { hero: { params: { prompt: 'product hero', aspect_ratio: '1:1' } } },
          },
          outputs: [{ id: 'art_1', nodeId: 'hero', kind: 'image' }],
        },
        editSession: null,
        appendManualEdit,
        queueRun,
      })) as never,
      { currentGeneration: () => 1, signal: () => undefined, isActive: () => true } as never,
    );

    await action('hero', 'make it night', '1:1');

    const proposal = appendManualEdit.mock.calls[0]?.[0] as { ops: Array<{ op: string; id?: string }> };
    expect(proposal.ops.some((op) => op.op === 'set_param' && op.id === 'hero')).toBe(false);
    expect(appendManualEdit).toHaveBeenCalledWith(expect.objectContaining({
      ops: expect.arrayContaining([
        expect.objectContaining({
          op: 'spawn_node',
          id: 'image_edit',
          node_type: 'image.edit',
          from: 'hero',
        }),
      ]),
    }));
    expect(queueRun).toHaveBeenCalledOnce();
  });

  it('spawns stacked video cards from a text card with duration and count', async () => {
    const appendManualEdit = vi.fn().mockResolvedValue(undefined);
    const queueRun = vi.fn().mockResolvedValue(undefined);
    const action = createGenerateFromMediaCardAction(
      vi.fn() as never,
      (() => ({
        state: {
          workspace: { id: 'ws_1', versionId: 'ver_1' },
          graph: {
            nodes: [{
              id: 'copy',
              nodeType: 'input.text',
              title: '文案',
              position: { x: 40, y: 80 },
              size: { width: 280, height: 280 },
            }],
            edges: [],
          },
          workflowGraph: { nodes: { copy: { params: { text: 'hero' } } } },
          outputs: [],
        },
        editSession: null,
        appendManualEdit,
        queueRun,
      })) as never,
      { currentGeneration: () => 1, signal: () => undefined, isActive: () => true } as never,
    );

    await action('copy', 'product video', '16:9', { mediaKind: 'video', durationSec: 8, count: 2 });

    const proposal = appendManualEdit.mock.calls[0]?.[0] as {
      ops: Array<{ op: string; id?: string; node_type?: string; from?: string; params?: { duration_sec?: number } }>;
    };
    const spawned = proposal.ops.filter((op) => op.op === 'spawn_node');
    expect(spawned).toHaveLength(2);
    expect(spawned.map((op) => op.id)).toEqual(['video_text_to_video', 'video_text_to_video_2']);
    expect(spawned.every((op) => op.node_type === 'video.text_to_video' && op.from === 'copy')).toBe(true);
    expect(spawned[0]?.params?.duration_sec).toBe(8);
    expect(queueRun).toHaveBeenCalledOnce();
  });
});

function mockExecution(
  intent: ImageProcessingJob['intent'],
  profile: string,
  model: string,
  width: number,
  height: number,
) {
  vi.mocked(createImageProcessingJob).mockResolvedValue({
    job: imageJob({ intent, profile, status: 'queued' }),
  });
  vi.mocked(waitForImageProcessingJob).mockResolvedValue(
    imageJob({
      intent,
      profile,
      model,
      provider: 'atlas',
      providerTaskId: 'provider_task_1',
      outputUploadId: 'upload_result',
      status: 'succeeded',
    }),
  );
  vi.mocked(linkImageProcessingResult).mockResolvedValue(imageJob({
      intent,
      profile,
      status: 'succeeded',
      providerTaskId: 'provider_task_1',
      provider: 'atlas',
      model,
      resultNodeId: 'image_outpaint',
      outputUploadId: 'upload_result',
    }));
  vi.stubGlobal('createImageBitmap', vi.fn().mockResolvedValue({ width, height, close: vi.fn() }));
}

function imageJob(
  overrides: Partial<ImageProcessingJob> = {},
): ImageProcessingJob {
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
