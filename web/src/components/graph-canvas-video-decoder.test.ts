import { afterEach, describe, expect, it } from 'vitest';
import {
  CANVAS_VIDEO_DECODER_LIMIT,
  CANVAS_VIDEO_DECODER_PRIORITY,
  flushCanvasVideoDecoders,
  isCanvasVideoDecoderActive,
  registerCanvasVideoDecoder,
  resetCanvasVideoDecoders,
  unregisterCanvasVideoDecoder,
} from './graph-canvas-video-decoder';

describe('canvas video decoder budget', () => {
  afterEach(() => {
    resetCanvasVideoDecoders();
  });

  it('gives the first visible cards a decoder and leaves the rest as shells', () => {
    for (let index = 0; index < CANVAS_VIDEO_DECODER_LIMIT + 8; index += 1) {
      registerCanvasVideoDecoder(`card-${index}`, CANVAS_VIDEO_DECODER_PRIORITY.visible);
    }
    flushCanvasVideoDecoders();

    expect(isCanvasVideoDecoderActive('card-0')).toBe(true);
    expect(isCanvasVideoDecoderActive(`card-${CANVAS_VIDEO_DECODER_LIMIT - 1}`)).toBe(true);
    expect(isCanvasVideoDecoderActive(`card-${CANVAS_VIDEO_DECODER_LIMIT}`)).toBe(false);
    expect(activeCount()).toBe(CANVAS_VIDEO_DECODER_LIMIT);
  });

  it('lets hover steal a slot from the lowest-ranked visible card', () => {
    for (let index = 0; index < CANVAS_VIDEO_DECODER_LIMIT + 1; index += 1) {
      registerCanvasVideoDecoder(`card-${index}`, CANVAS_VIDEO_DECODER_PRIORITY.visible);
    }
    registerCanvasVideoDecoder(
      `card-${CANVAS_VIDEO_DECODER_LIMIT}`,
      CANVAS_VIDEO_DECODER_PRIORITY.hover,
    );
    flushCanvasVideoDecoders();

    expect(isCanvasVideoDecoderActive(`card-${CANVAS_VIDEO_DECODER_LIMIT}`)).toBe(true);
    expect(isCanvasVideoDecoderActive('card-0')).toBe(true);
    expect(isCanvasVideoDecoderActive(`card-${CANVAS_VIDEO_DECODER_LIMIT - 1}`)).toBe(false);
    expect(activeCount()).toBe(CANVAS_VIDEO_DECODER_LIMIT);
  });

  it('keeps a selected card ahead of later visible cards', () => {
    registerCanvasVideoDecoder('selected', CANVAS_VIDEO_DECODER_PRIORITY.selected);
    for (let index = 0; index < CANVAS_VIDEO_DECODER_LIMIT; index += 1) {
      registerCanvasVideoDecoder(`card-${index}`, CANVAS_VIDEO_DECODER_PRIORITY.visible);
    }
    flushCanvasVideoDecoders();

    expect(isCanvasVideoDecoderActive('selected')).toBe(true);
    expect(isCanvasVideoDecoderActive('card-0')).toBe(true);
    expect(isCanvasVideoDecoderActive(`card-${CANVAS_VIDEO_DECODER_LIMIT - 1}`)).toBe(false);
  });

  it('frees a slot when a card unmounts', () => {
    for (let index = 0; index < CANVAS_VIDEO_DECODER_LIMIT + 1; index += 1) {
      registerCanvasVideoDecoder(`card-${index}`, CANVAS_VIDEO_DECODER_PRIORITY.visible);
    }
    flushCanvasVideoDecoders();
    unregisterCanvasVideoDecoder('card-0');
    flushCanvasVideoDecoders();

    expect(isCanvasVideoDecoderActive('card-0')).toBe(false);
    expect(isCanvasVideoDecoderActive(`card-${CANVAS_VIDEO_DECODER_LIMIT}`)).toBe(true);
  });
});

function activeCount(): number {
  let count = 0;
  for (let index = 0; index < 64; index += 1) {
    if (isCanvasVideoDecoderActive(`card-${index}`)) count += 1;
  }
  if (isCanvasVideoDecoderActive('selected')) count += 1;
  return count;
}
