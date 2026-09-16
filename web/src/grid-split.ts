import { graphNodeHeight, graphNodeWidth } from './components/graph-canvas-navigation';
import imageProcessors from './cuter-image-processors';
import type { GraphNodeState, WorkbenchState } from './types';

export type GridSplitSource =
  | { kind: 'upload'; uploadId: string }
  | { kind: 'artifact'; artifactId: string };

export type GridSplitTilePlacement = {
  row: number;
  column: number;
  x: number;
  y: number;
};

export const TAPNOW_SPLIT_PRESETS = [
  { rows: 2, columns: 2, label: '2×2' },
  { rows: 3, columns: 3, label: '3×3' },
  { rows: 4, columns: 4, label: '4×4' },
] as const;

export type MediaNodeType = 'input.image' | 'input.video' | 'input.audio';

export type PixelCropRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

const GRID_SPLIT_GAP = 24;
const UPLOAD_PREFIX = 'upload://';

export function nodeTypeFromMime(mime: string): MediaNodeType {
  if (mime.startsWith('video/')) return 'input.video';
  if (mime.startsWith('audio/')) return 'input.audio';
  return 'input.image';
}

export function isMediaNodeType(nodeType: string): nodeType is MediaNodeType {
  return nodeType === 'input.image' || nodeType === 'input.video' || nodeType === 'input.audio';
}

export function isVisualMediaCardType(nodeType: string): boolean {
  return (
    isMediaNodeType(nodeType) ||
    nodeType === 'image.generate' ||
    nodeType === 'image.edit'
  );
}

export function isCanvasCardType(nodeType: string): boolean {
  return isVisualMediaCardType(nodeType) || nodeType === 'input.text';
}

export function parseUploadUri(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed.startsWith(UPLOAD_PREFIX)) return null;
  const uploadId = trimmed.slice(UPLOAD_PREFIX.length).trim();
  if (!uploadId || uploadId.includes('/') || uploadId.includes('\\')) return null;
  return uploadId;
}

export function mediaKindLabel(nodeType: string): string {
  if (nodeType === 'input.video' || nodeType.startsWith('video.')) return '视频';
  if (nodeType === 'input.audio') return '音频';
  return '图片';
}

export async function readNaturalImageSize(
  file: Blob,
): Promise<{ width: number; height: number } | undefined> {
  if (typeof createImageBitmap === 'function') {
    try {
      const bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' });
      const size = { width: bitmap.width, height: bitmap.height };
      bitmap.close();
      if (size.width > 0 && size.height > 0) return size;
    } catch {
      try {
        const bitmap = await createImageBitmap(file);
        const size = { width: bitmap.width, height: bitmap.height };
        bitmap.close();
        if (size.width > 0 && size.height > 0) return size;
      } catch {
        // Fall through to HTMLImageElement; some drag-drop blobs have an empty MIME.
      }
    }
  }
  return readNaturalImageSizeFromElement(file);
}

function readNaturalImageSizeFromElement(
  file: Blob,
): Promise<{ width: number; height: number } | undefined> {
  if (typeof Image === 'undefined' || typeof URL === 'undefined') return Promise.resolve(undefined);
  return new Promise((resolve) => {
    const url = URL.createObjectURL(file);
    const image = new Image();
    image.onload = () => {
      const size = { width: image.naturalWidth, height: image.naturalHeight };
      URL.revokeObjectURL(url);
      resolve(size.width > 0 && size.height > 0 ? size : undefined);
    };
    image.onerror = () => {
      URL.revokeObjectURL(url);
      resolve(undefined);
    };
    image.src = url;
  });
}

export function fitMediaNodeSize(width: number, height: number): { width: number; height: number } {
  const maxWidth = 360;
  const maxHeight = 280;
  const minWidth = 180;
  const minHeight = 140;
  const safeWidth = Math.max(1, width);
  const safeHeight = Math.max(1, height);
  const scale = Math.min(maxWidth / safeWidth, maxHeight / safeHeight, 1);
  return {
    width: Math.round(Math.max(minWidth, Math.min(maxWidth, safeWidth * scale))),
    height: Math.round(Math.max(minHeight, Math.min(maxHeight, safeHeight * scale))),
  };
}

export function clampPixelCrop(
  crop: PixelCropRect,
  imageWidth: number,
  imageHeight: number,
): PixelCropRect {
  const width = Math.max(1, Math.min(imageWidth, Math.round(crop.width)));
  const height = Math.max(1, Math.min(imageHeight, Math.round(crop.height)));
  const x = Math.max(0, Math.min(imageWidth - width, Math.round(crop.x)));
  const y = Math.max(0, Math.min(imageHeight - height, Math.round(crop.y)));
  return { x, y, width, height };
}

export async function cropImageBlob(
  blob: Blob,
  crop: PixelCropRect,
): Promise<{ blob: Blob; width: number; height: number }> {
  const bitmap = await createImageBitmap(blob);
  try {
    const rect = clampPixelCrop(crop, bitmap.width, bitmap.height);
    const canvas = document.createElement('canvas');
    canvas.width = rect.width;
    canvas.height = rect.height;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('无法创建裁剪画布');
    context.drawImage(
      bitmap,
      rect.x,
      rect.y,
      rect.width,
      rect.height,
      0,
      0,
      rect.width,
      rect.height,
    );
    const cropped = await new Promise<Blob>((resolve, reject) => {
      canvas.toBlob((next) => {
        if (next) resolve(next);
        else reject(new Error('裁剪导出失败'));
      }, 'image/png');
    });
    return { blob: cropped, width: rect.width, height: rect.height };
  } finally {
    bitmap.close();
  }
}

export function validateGridSplitAxes(rows: number, columns: number): void {
  const dummy = Math.max(rows, columns, 2);
  imageProcessors.createGridTiles(dummy, dummy, rows, columns);
}

export function resolveGridSplitSource(input: {
  nodeType: string;
  params: unknown;
  artifacts: Array<{ id: string; kind: string }>;
}): GridSplitSource | null {
  const uploadId = parseUploadUri(stringParam(input.params, 'storage_uri'));
  if (input.nodeType === 'input.image' && uploadId) {
    return { kind: 'upload', uploadId };
  }

  const image = input.artifacts.find((artifact) => artifact.kind === 'image');
  if (image?.id) return { kind: 'artifact', artifactId: image.id };
  return null;
}

export function gridSplitFilename(stem: string, row: number, column: number): string {
  return `${sanitizeFilename(stem)}-grid-r${row + 1}c${column + 1}.png`;
}

export function gridSplitTilePositions(input: {
  source: GraphNodeState;
  rows: number;
  columns: number;
  tileWidth: number;
  tileHeight: number;
}): GridSplitTilePlacement[] {
  validateGridSplitAxes(input.rows, input.columns);
  const originX = input.source.position.x + graphNodeWidth(input.source) + GRID_SPLIT_GAP;
  const originY = input.source.position.y;
  const stepX = input.tileWidth + GRID_SPLIT_GAP;
  const stepY = input.tileHeight + GRID_SPLIT_GAP;
  const placements: GridSplitTilePlacement[] = [];
  for (let row = 0; row < input.rows; row += 1) {
    for (let column = 0; column < input.columns; column += 1) {
      placements.push({
        row,
        column,
        x: originX + column * stepX,
        y: originY + row * stepY,
      });
    }
  }
  return placements;
}

export function artifactsForNode(
  outputs: WorkbenchState['outputs'] | undefined,
  nodeId: string,
): Array<{ id: string; kind: string }> {
  return (outputs ?? [])
    .filter((output) => output.nodeId === nodeId)
    .map((output) => ({ id: output.id, kind: output.kind }));
}

export async function fetchArtifactBlob(outputId: string, signal?: AbortSignal): Promise<Blob> {
  const response = await fetch(`/api/artifacts/${encodeURIComponent(outputId)}/content`, { signal });
  if (!response.ok) {
    throw new Error(`artifact content request failed: ${response.status}`);
  }
  return response.blob();
}

export async function fetchWorkspaceUploadContent(
  workspaceId: string,
  uploadId: string,
  signal?: AbortSignal,
): Promise<Blob> {
  const response = await fetch(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/uploads/${encodeURIComponent(uploadId)}/content`,
    { signal },
  );
  if (!response.ok) {
    throw new Error(`upload content request failed: ${response.status}`);
  }
  return response.blob();
}

function stringParam(params: unknown, key: string): string | null {
  if (!params || typeof params !== 'object' || Array.isArray(params)) return null;
  const value = (params as Record<string, unknown>)[key];
  return typeof value === 'string' && value.trim() ? value.trim() : null;
}

function sanitizeFilename(value: string): string {
  const stem = value.trim().replace(/[/\\?%*:|"<>]/g, '-').replace(/\s+/g, '-');
  return stem || 'image';
}
