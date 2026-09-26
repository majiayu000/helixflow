export type ImageCanvasToolKind = 'outpaint' | 'inpaint' | 'cutout' | 'upscale' | 'enhance';

export type ImageGenerationOptions = {
  profile: string;
  quality?: 'low' | 'medium' | 'high';
  sizeTier?: '1k' | '2k' | '4k';
};

export type ImageCanvasToolRequest =
  | {
      kind: 'outpaint';
      left: number;
      top: number;
      right: number;
      bottom: number;
      prompt: string;
    } & ImageGenerationOptions
  | {
      kind: 'inpaint';
      x: number;
      y: number;
      width: number;
      height: number;
      prompt: string;
      maskBlob?: Blob;
    } & ImageGenerationOptions
  | {
      kind: 'cutout';
    }
  | {
      kind: 'upscale';
      scale: 2 | 4;
    }
  | {
      kind: 'enhance';
    };

export const DEFAULT_IMAGE_EDIT_PROMPTS = {
  outpaint:
    'Fill only the transparent padded border. Keep the original subject, lighting, and details unchanged.',
  inpaint:
    'Fill only the transparent region. Match surrounding texture, lighting, and perspective. Do not change unmasked pixels.',
  redraw:
    'Redraw only the masked region according to the prompt. Keep unmasked pixels unchanged.',
} as const;

export function defaultImageCanvasToolRequest(kind: ImageCanvasToolKind): ImageCanvasToolRequest {
  if (kind === 'outpaint') {
    return {
      kind,
      left: 64,
      top: 64,
      right: 64,
      bottom: 64,
      prompt: DEFAULT_IMAGE_EDIT_PROMPTS.outpaint,
      profile: '',
    };
  }
  if (kind === 'inpaint') {
    return {
      kind,
      x: 0,
      y: 0,
      width: 64,
      height: 64,
      prompt: DEFAULT_IMAGE_EDIT_PROMPTS.inpaint,
      profile: '',
    };
  }
  if (kind === 'upscale') {
    return { kind, scale: 2 };
  }
  if (kind === 'enhance') {
    return { kind };
  }
  return { kind };
}

export function imageCanvasToolLabel(kind: ImageCanvasToolKind): string {
  if (kind === 'outpaint') return '扩图';
  if (kind === 'inpaint') return '擦除';
  if (kind === 'upscale') return '超分';
  if (kind === 'enhance') return '增强';
  return '抠图';
}

export function imageCanvasPrepFilename(stem: string, kind: ImageCanvasToolKind): string {
  const safe = stem.trim().replace(/[/\\?%*:|"<>]/g, '-').replace(/\s+/g, '-') || 'image';
  if (kind === 'outpaint') return `${safe}-outpaint-canvas.png`;
  if (kind === 'inpaint') return `${safe}-inpaint-canvas.png`;
  if (kind === 'upscale') return `${safe}-upscale.png`;
  if (kind === 'enhance') return `${safe}-enhance.png`;
  return `${safe}-cutout.png`;
}

export function imageCanvasResultFilename(stem: string, kind: ImageCanvasToolKind): string {
  const safe = stem.trim().replace(/[/\\?%*:|"<>]/g, '-').replace(/\s+/g, '-') || 'image';
  return `${safe}-${kind}.png`;
}

export function needsPreparedCanvas(
  kind: ImageCanvasToolKind,
): kind is 'outpaint' | 'inpaint' {
  return kind === 'outpaint' || kind === 'inpaint';
}

export async function punchEraseMask(
  blob: Blob,
  mask: Uint8Array,
  width: number,
  height: number,
): Promise<Blob> {
  if (mask.length !== width * height) {
    throw new Error('擦除蒙版尺寸不匹配');
  }
  const bitmap = await createImageBitmap(blob);
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  if (!context) {
    bitmap.close();
    throw new Error('无法创建擦除画布');
  }
  context.drawImage(bitmap, 0, 0, width, height);
  bitmap.close();
  const image = context.getImageData(0, 0, width, height);
  for (let index = 0; index < mask.length; index += 1) {
    if (mask[index]) image.data[index * 4 + 3] = 0;
  }
  context.putImageData(image, 0, 0);
  const punched = await new Promise<Blob>((resolve, reject) => {
    canvas.toBlob((next) => {
      if (next) resolve(next);
      else reject(new Error('擦除画布导出失败'));
    }, 'image/png');
  });
  return punched;
}
