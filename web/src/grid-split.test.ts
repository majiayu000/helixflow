import { describe, expect, it } from 'vitest';
import imageProcessors from './cuter-image-processors';
import { MEDIA_CARD_HEIGHT, MEDIA_CARD_WIDTH } from './components/graph-canvas-navigation';
import {
  artifactsForNode,
  clampPixelCrop,
  fitMediaNodeSize,
  gridSplitFilename,
  gridSplitTilePositions,
  nodeTypeFromMime,
  mediaKindLabel,
  parseUploadUri,
  readNaturalImageSize,
  resolveGridSplitSource,
  validateGridSplitAxes,
} from './grid-split';
import type { GraphNodeState } from './types';

describe('cuter image-processors sdk', () => {
  it('reuses Cuter createGridTiles geometry without copying the algorithm', () => {
    const tiles = imageProcessors.createGridTiles(7, 5, 2, 3);
    expect(tiles.map(({ x, y, width, height }) => ({ x, y, width, height }))).toEqual([
      { x: 0, y: 0, width: 2, height: 2 },
      { x: 2, y: 0, width: 2, height: 2 },
      { x: 4, y: 0, width: 3, height: 2 },
      { x: 0, y: 2, width: 2, height: 3 },
      { x: 2, y: 2, width: 2, height: 3 },
      { x: 4, y: 2, width: 3, height: 3 },
    ]);
    expect(tiles.reduce((sum, tile) => sum + tile.width * tile.height, 0)).toBe(35);
  });
});

describe('media ingest helpers', () => {
  it('maps mime types onto TapNow-style media cards', () => {
    expect(mediaKindLabel('input.image')).toBe('图片');
    expect(mediaKindLabel('image.generate')).toBe('图片');
    expect(mediaKindLabel('input.video')).toBe('视频');
    expect(mediaKindLabel('input.audio')).toBe('音频');
    expect(nodeTypeFromMime('image/png')).toBe('input.image');
    expect(nodeTypeFromMime('video/mp4')).toBe('input.video');
    expect(nodeTypeFromMime('audio/mpeg')).toBe('input.audio');
    expect(parseUploadUri('upload://abc')).toBe('abc');
    expect(parseUploadUri('upload://a/b')).toBeNull();
  });

  it('reads pixel size even when drag-drop files have an empty mime', async () => {
    const file = new File([new Uint8Array([1, 2, 3])], 'photo.jpg', { type: '' });
    const original = globalThis.createImageBitmap;
    globalThis.createImageBitmap = (async () => ({
      width: 1920,
      height: 1080,
      close() {},
    })) as typeof createImageBitmap;
    try {
      await expect(readNaturalImageSize(file)).resolves.toEqual({ width: 1920, height: 1080 });
    } finally {
      if (original) globalThis.createImageBitmap = original;
      else delete (globalThis as { createImageBitmap?: typeof createImageBitmap }).createImageBitmap;
    }
  });

  it('fits media cards and clamps crop rectangles', () => {
    expect(fitMediaNodeSize(1920, 1080)).toEqual({ width: 360, height: 203 });
    expect(clampPixelCrop({ x: -10, y: 10, width: 400, height: 20 }, 100, 50)).toEqual({
      x: 0,
      y: 10,
      width: 100,
      height: 20,
    });
  });
});

describe('grid split helpers', () => {
  it('resolves uploaded input.image nodes and image artifacts', () => {
    expect(
      resolveGridSplitSource({
        nodeType: 'input.image',
        params: { storage_uri: 'upload://upload_abc' },
        artifacts: [],
      }),
    ).toEqual({ kind: 'upload', uploadId: 'upload_abc' });
    expect(
      resolveGridSplitSource({
        nodeType: 'image.generate',
        params: {},
        artifacts: [{ id: 'art_1', kind: 'video' }, { id: 'art_2', kind: 'image' }],
      }),
    ).toEqual({ kind: 'artifact', artifactId: 'art_2' });
    expect(
      resolveGridSplitSource({
        nodeType: 'input.image',
        params: { storage_uri: 'upload://../secret' },
        artifacts: [],
      }),
    ).toBeNull();
  });

  it('rejects illegal 宫格 axes with the Cuter processor errors', () => {
    expect(() => validateGridSplitAxes(1, 1)).toThrow(/2 到 100/);
    expect(() => validateGridSplitAxes(11, 1)).toThrow(RangeError);
  });

  it('places tiles to the right of the source node in row-major order', () => {
    const source = imageNode();
    const originX = source.position.x + MEDIA_CARD_WIDTH + 24;
    const stepX = MEDIA_CARD_WIDTH + 24;
    const stepY = MEDIA_CARD_HEIGHT + 24;
    const placements = gridSplitTilePositions({
      source,
      rows: 2,
      columns: 2,
      tileWidth: MEDIA_CARD_WIDTH,
      tileHeight: MEDIA_CARD_HEIGHT,
    });
    expect(placements).toEqual([
      { row: 0, column: 0, x: originX, y: 40 },
      { row: 0, column: 1, x: originX + stepX, y: 40 },
      { row: 1, column: 0, x: originX, y: 40 + stepY },
      { row: 1, column: 1, x: originX + stepX, y: 40 + stepY },
    ]);
  });

  it('spaces tiles by tile card size so a smaller source does not overlap them', () => {
    const source = { ...imageNode(), size: { width: 200, height: 200 } };
    const placements = gridSplitTilePositions({
      source,
      rows: 2,
      columns: 2,
      tileWidth: MEDIA_CARD_WIDTH,
      tileHeight: MEDIA_CARD_HEIGHT,
    });
    expect(placements[0]).toEqual({
      row: 0,
      column: 0,
      x: source.position.x + 200 + 24,
      y: source.position.y,
    });
    expect(placements[1]!.x - placements[0]!.x).toBe(MEDIA_CARD_WIDTH + 24);
    expect(placements[2]!.y - placements[0]!.y).toBe(MEDIA_CARD_HEIGHT + 24);
  });

  it('names tiles the same way Cuter imports them', () => {
    expect(gridSplitFilename('产品主图', 0, 2)).toBe('产品主图-grid-r1c3.png');
  });

  it('filters artifacts by node id', () => {
    expect(
      artifactsForNode(
        [
          { id: 'a', kind: 'image', nodeId: 'n1' },
          { id: 'b', kind: 'image', nodeId: 'n2' },
        ] as never,
        'n1',
      ),
    ).toEqual([{ id: 'a', kind: 'image' }]);
  });
});

function imageNode(): GraphNodeState {
  return {
    id: 'photo',
    nodeType: 'input.image',
    title: '产品主图',
    category: 'Input',
    status: 'queued',
    position: { x: 240, y: 40 },
    provider: null,
    summary: 'input.image',
  };
}
