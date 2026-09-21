import { describe, expect, it, vi } from 'vitest';
import { uploadWorkspaceImage } from './api';
import { createImageMediaActions } from './store-upload';
import { captureVideoFrameBlob, liveCanvasVideo } from './video-frame';

vi.mock('./video-frame', () => ({
  captureVideoFrameBlob: vi.fn(),
  liveCanvasVideo: vi.fn(),
}));

vi.mock('./api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./api')>()),
  uploadWorkspaceImage: vi.fn(),
}));

describe('extractVideoFrame', () => {
  it('uploads the captured frame as a new image card without lineage', async () => {
    const appendManualEdit = vi.fn().mockResolvedValue(undefined);
    vi.mocked(liveCanvasVideo).mockReturnValue({
      currentSrc: 'blob:clip',
      currentTime: 1.25,
    } as HTMLVideoElement);
    vi.mocked(captureVideoFrameBlob).mockResolvedValue({
      blob: new Blob(['frame'], { type: 'image/jpeg' }),
      width: 1920,
      height: 1080,
    });
    vi.mocked(uploadWorkspaceImage).mockResolvedValue({
      id: 'up_frame',
      storageUri: 'upload://frame',
      filename: 'clip-last.jpg',
      mime: 'image/jpeg',
    });
    const actions = createImageMediaActions(
      vi.fn() as never,
      (() => ({
        state: {
          workspace: { id: 'ws_1', versionId: 'ver_1' },
          graph: {
            nodes: [{
              id: 'clip',
              nodeType: 'input.video',
              title: '成片',
              position: { x: 80, y: 40 },
              size: { width: 320, height: 180 },
            }],
            edges: [],
          },
          workflowGraph: { nodes: { clip: { params: { storage_uri: 'upload://clip' } } } },
          outputs: [],
        },
        editSession: null,
        appendManualEdit,
      })) as never,
      { currentGeneration: () => 1, signal: () => undefined, isActive: () => true } as never,
    );

    await actions.extractVideoFrame('clip', 'last');

    expect(captureVideoFrameBlob).toHaveBeenCalledWith({
      url: 'blob:clip',
      kind: 'last',
      currentTime: 1.25,
      live: expect.objectContaining({ currentSrc: 'blob:clip' }),
    });
    expect(appendManualEdit).toHaveBeenCalledWith(expect.objectContaining({
      ops: expect.arrayContaining([
        expect.objectContaining({
          op: 'spawn_node',
          id: 'image_frame_last',
          node_type: 'input.image',
          title: '成片 末帧',
          params: { storage_uri: 'upload://frame' },
        }),
      ]),
    }));
    const proposal = appendManualEdit.mock.calls[0]?.[0] as { ops: Array<{ from?: string }> };
    expect(proposal.ops.some((op) => op.from)).toBe(false);
  });
});
