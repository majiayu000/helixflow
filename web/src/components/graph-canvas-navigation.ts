import type { GraphNodeState } from '../types';

export type ViewState = {
  x: number;
  y: number;
  z: number;
};

export type ViewportSize = {
  width: number;
  height: number;
};

export type MinimapLayout = {
  width: number;
  height: number;
  worldBounds: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
  scale: number;
  offset: {
    x: number;
    y: number;
  };
  nodes: Array<{
    id: string;
    x: number;
    y: number;
    width: number;
    height: number;
  }>;
};

type CanvasViewStorage = Pick<Storage, 'getItem' | 'setItem'>;

export const DEFAULT_GRAPH_VIEW: ViewState = { x: 72, y: 98, z: 0.78 };
export const GRAPH_CANVAS_VIEW_STORAGE_PREFIX = 'helixflow:graph-canvas-view:v2:';
export const GRAPH_NODE_WIDTH = 240;
export const GRAPH_NODE_HEAD_HEIGHT = 43;
export const GRAPH_NODE_ROW_HEIGHT = 32;
export const GRAPH_NODE_MIN_WIDTH = 180;
export const GRAPH_NODE_MIN_HEIGHT = 116;
export const GRAPH_NODE_MAX_WIDTH = 420;
export const GRAPH_NODE_MAX_HEIGHT = 360;
export const MINIMAP_WIDTH = 188;
export const MINIMAP_HEIGHT = 124;

const minZoom = 0.4;
const maxZoom = 1.4;
const minimapPadding = 260;

export function viewStorageKey(workspaceId: string): string {
  return `${GRAPH_CANVAS_VIEW_STORAGE_PREFIX}${workspaceId}`;
}

export function loadGraphCanvasView(workspaceId: string): ViewState {
  const storage = safeLocalStorage();
  if (!storage || !workspaceId) return DEFAULT_GRAPH_VIEW;
  try {
    return normalizeView(JSON.parse(storage.getItem(viewStorageKey(workspaceId)) ?? 'null'));
  } catch {
    return DEFAULT_GRAPH_VIEW;
  }
}

export function saveGraphCanvasView(workspaceId: string, view: ViewState): void {
  const storage = safeLocalStorage();
  if (!storage || !workspaceId) return;
  try {
    storage.setItem(viewStorageKey(workspaceId), JSON.stringify(normalizeView(view)));
  } catch {
    // Persistence is best-effort; quota and privacy-mode failures must not break the canvas.
  }
}

export function zoomViewAtPoint(
  view: ViewState,
  input: { deltaY: number; localX: number; localY: number },
): ViewState {
  const current = normalizeView(view);
  const nextZ = clampZoom(current.z * Math.pow(1.1, -input.deltaY / 100));
  const worldX = (input.localX - current.x) / current.z;
  const worldY = (input.localY - current.y) / current.z;
  return normalizeView({
    x: input.localX - worldX * nextZ,
    y: input.localY - worldY * nextZ,
    z: nextZ,
  });
}

export function clampZoom(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_GRAPH_VIEW.z;
  return Math.min(maxZoom, Math.max(minZoom, value));
}

export function normalizeView(value: unknown): ViewState {
  if (!isRecord(value)) return DEFAULT_GRAPH_VIEW;
  const x = typeof value.x === 'number' && Number.isFinite(value.x) ? value.x : DEFAULT_GRAPH_VIEW.x;
  const y = typeof value.y === 'number' && Number.isFinite(value.y) ? value.y : DEFAULT_GRAPH_VIEW.y;
  const z = typeof value.z === 'number' && Number.isFinite(value.z) ? value.z : DEFAULT_GRAPH_VIEW.z;
  return { x, y, z: clampZoom(z) };
}

export function computeMinimapLayout(
  nodes: GraphNodeState[],
  size: ViewportSize = { width: MINIMAP_WIDTH, height: MINIMAP_HEIGHT },
): MinimapLayout | null {
  const measured = nodes
    .map((node) => ({
      id: node.id,
      x: node.position.x,
      y: node.position.y,
      width: graphNodeWidth(node),
      height: graphNodeHeight(node),
    }))
    .filter((node) =>
      [node.x, node.y, node.width, node.height].every((item) => Number.isFinite(item)),
    );
  if (measured.length === 0) return null;

  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  measured.forEach((node) => {
    minX = Math.min(minX, node.x);
    minY = Math.min(minY, node.y);
    maxX = Math.max(maxX, node.x + node.width);
    maxY = Math.max(maxY, node.y + node.height);
  });

  const worldBounds = {
    x: minX - minimapPadding,
    y: minY - minimapPadding,
    width: Math.max(1, maxX - minX + minimapPadding * 2),
    height: Math.max(1, maxY - minY + minimapPadding * 2),
  };
  const scale = Math.min(size.width / worldBounds.width, size.height / worldBounds.height);
  const contentWidth = worldBounds.width * scale;
  const contentHeight = worldBounds.height * scale;
  const offset = {
    x: (size.width - contentWidth) / 2,
    y: (size.height - contentHeight) / 2,
  };

  return {
    width: size.width,
    height: size.height,
    worldBounds,
    scale,
    offset,
    nodes: measured.map((node) => ({
      id: node.id,
      x: (node.x - worldBounds.x) * scale + offset.x,
      y: (node.y - worldBounds.y) * scale + offset.y,
      width: Math.max(2, node.width * scale),
      height: Math.max(2, node.height * scale),
    })),
  };
}

export function viewForMinimapPoint(
  layout: MinimapLayout,
  point: { x: number; y: number },
  view: ViewState,
  viewportSize: ViewportSize,
): ViewState {
  const worldX = (point.x - layout.offset.x) / layout.scale + layout.worldBounds.x;
  const worldY = (point.y - layout.offset.y) / layout.scale + layout.worldBounds.y;
  return normalizeView({
    x: viewportSize.width / 2 - worldX * view.z,
    y: viewportSize.height / 2 - worldY * view.z,
    z: view.z,
  });
}

export function minimapViewportRect(
  layout: MinimapLayout,
  view: ViewState,
  viewportSize: ViewportSize,
) {
  const left = -view.x / view.z;
  const top = -view.y / view.z;
  const width = viewportSize.width / view.z;
  const height = viewportSize.height / view.z;
  const x = (left - layout.worldBounds.x) * layout.scale + layout.offset.x;
  const y = (top - layout.worldBounds.y) * layout.scale + layout.offset.y;
  return {
    x,
    y,
    width: Math.max(4, width * layout.scale),
    height: Math.max(4, height * layout.scale),
  };
}

export function graphNodeWidth(node: GraphNodeState): number {
  const width = node.size?.width;
  return typeof width === 'number' && Number.isFinite(width)
    ? Math.min(GRAPH_NODE_MAX_WIDTH, Math.max(GRAPH_NODE_MIN_WIDTH, width))
    : GRAPH_NODE_WIDTH;
}

export function graphNodeHeight(node: GraphNodeState): number {
  const height = node.size?.height;
  const contentHeight =
    GRAPH_NODE_HEAD_HEIGHT +
    GRAPH_NODE_ROW_HEIGHT +
    Math.max(1, Math.min(4, summaryParamCount(node.summary))) * GRAPH_NODE_ROW_HEIGHT;
  return typeof height === 'number' && Number.isFinite(height)
    ? Math.min(GRAPH_NODE_MAX_HEIGHT, Math.max(contentHeight, height))
    : contentHeight;
}

function summaryParamCount(summary: string): number {
  if (!summary || summary === '{}') return 0;
  try {
    const parsed = JSON.parse(summary);
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
      ? Object.keys(parsed).length
      : 1;
  } catch {
    return 1;
  }
}

function safeLocalStorage(): CanvasViewStorage | null {
  try {
    const storage = globalThis.localStorage as Partial<Storage> | undefined;
    return storage &&
      typeof storage.getItem === 'function' &&
      typeof storage.setItem === 'function'
      ? (storage as CanvasViewStorage)
      : null;
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
