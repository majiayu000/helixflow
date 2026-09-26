import type { GraphNodeState, WorkbenchState } from '../types';
import {
  DEFAULT_GRAPH_VIEW,
  clampZoom,
  graphNodeHeight,
  graphNodeWidth,
  type ViewState,
  type ViewportSize,
} from './graph-canvas-navigation';

export type Point = { x: number; y: number };
export type Rect = { x: number; y: number; width: number; height: number };
export type GraphShortcut =
  | 'clear_selection'
  | 'select_all'
  | 'fit_view'
  | 'find_nodes'
  | 'zoom_in'
  | 'zoom_out'
  | 'copy_selection'
  | 'paste_selection'
  | 'duplicate_selection'
  | 'group_selection'
  | 'ungroup_selection'
  | 'delete_selection'
  | 'undo'
  | 'redo';

export function selectionRectFromPoints(start: Point, current: Point): Rect {
  return {
    x: Math.min(start.x, current.x),
    y: Math.min(start.y, current.y),
    width: Math.abs(current.x - start.x),
    height: Math.abs(current.y - start.y),
  };
}

export function worldRectFromLocalRect(rect: Rect, view: ViewState): Rect {
  return {
    x: (rect.x - view.x) / view.z,
    y: (rect.y - view.y) / view.z,
    width: rect.width / view.z,
    height: rect.height / view.z,
  };
}

export function selectedIdsInWorldRect(nodes: GraphNodeState[], rect: Rect): Set<string> {
  const selected = new Set<string>();
  for (const node of nodes) {
    const nodeRect = {
      x: node.position.x,
      y: node.position.y,
      width: graphNodeWidth(node),
      height: graphNodeHeight(node),
    };
    if (rectsIntersect(rect, nodeRect)) {
      selected.add(node.id);
    }
  }
  return selected;
}

export function mergeSelection(
  baseIds: ReadonlySet<string>,
  hitIds: ReadonlySet<string>,
  additive: boolean,
): Set<string> {
  if (!additive) return new Set(hitIds);
  const next = new Set(baseIds);
  for (const id of hitIds) {
    next.add(id);
  }
  return next;
}

export function fitViewToNodes(nodes: GraphNodeState[], viewportSize: ViewportSize): ViewState {
  const measured = nodes.filter((node) =>
    [node.position.x, node.position.y].every((item) => Number.isFinite(item)),
  );
  if (measured.length === 0 || viewportSize.width <= 0 || viewportSize.height <= 0) {
    return DEFAULT_GRAPH_VIEW;
  }

  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const node of measured) {
    minX = Math.min(minX, node.position.x);
    minY = Math.min(minY, node.position.y);
    maxX = Math.max(maxX, node.position.x + graphNodeWidth(node));
    maxY = Math.max(maxY, node.position.y + graphNodeHeight(node));
  }
  const padding = 72;
  const width = Math.max(1, maxX - minX + padding * 2);
  const height = Math.max(1, maxY - minY + padding * 2);
  const z = clampZoom(Math.min(viewportSize.width / width, viewportSize.height / height));
  return {
    x: viewportSize.width / 2 - (minX + (maxX - minX) / 2) * z,
    y: viewportSize.height / 2 - (minY + (maxY - minY) / 2) * z,
    z,
  };
}

export function graphShortcutFromEvent(event: {
  key: string;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  nativeEvent?: { isComposing?: boolean; keyCode?: number; which?: number };
}): GraphShortcut | null {
  if (isImeEvent(event.nativeEvent)) return null;
  const key = event.key.toLowerCase();
  const command = Boolean(event.metaKey || event.ctrlKey);
  if (key === 'escape') return 'clear_selection';
  if (!command && (key === 'delete' || key === 'backspace')) return 'delete_selection';
  if (!command) return null;
  if (key === 'a') return 'select_all';
  if (key === '0') return 'fit_view';
  if (key === 'f') return 'find_nodes';
  if (key === '=' || key === '+') return 'zoom_in';
  if (key === '-') return 'zoom_out';
  if (key === 'c') return 'copy_selection';
  if (key === 'v') return 'paste_selection';
  if (key === 'd') return 'duplicate_selection';
  if (key === 'g' && event.shiftKey) return 'ungroup_selection';
  if (key === 'g') return 'group_selection';
  if (key === 'z' && event.shiftKey) return 'redo';
  if (key === 'z') return 'undo';
  if (key === 'y') return 'redo';
  return null;
}

export function isEditableShortcutTarget(target: EventTarget | null): boolean {
  if (typeof Element === 'undefined') return false;
  if (!(target instanceof Element)) return false;
  return Boolean(target.closest('input,textarea,select,[contenteditable="true"],.composer,.wb-chat'));
}

export function selectionClipboardText(
  nodes: GraphNodeState[],
  edges: WorkbenchState['graph']['edges'],
): string {
  const selectedIds = new Set(nodes.map((node) => node.id));
  const graph = {
    schema_version: 1,
    nodes: Object.fromEntries(
      nodes
        .slice()
        .sort((left, right) => left.id.localeCompare(right.id))
        .map((node) => [
          node.id,
          {
            node_type: node.nodeType,
            title: sanitizeClipboardText(node.title),
            category: node.category,
            summary: sanitizeClipboardText(node.summary),
            pos: [node.position.x, node.position.y],
          },
        ]),
    ),
    edges: edges
      .filter((edge) => selectedIds.has(edge.from.nodeId) && selectedIds.has(edge.to.nodeId))
      .map((edge) => ({
        from: [edge.from.nodeId, edge.from.port],
        to: [edge.to.nodeId, edge.to.port],
        edge_type: edge.kind,
      })),
  };
  const labels = nodes.map((node) => `${node.id}: ${sanitizeClipboardText(node.title)}`).join('\n');
  return `Helixflow selection (${nodes.length} nodes)\n${labels}\n\n${JSON.stringify(graph, null, 2)}`;
}

export function sanitizeClipboardText(value: string): string {
  return value
    .replace(/sk-[A-Za-z0-9_-]{8,}/g, '[redacted-secret]')
    .replace(/\b[A-Za-z0-9_-]*(?:token|secret|password|apikey|api_key)[A-Za-z0-9_-]*\b/gi, '[redacted-key]')
    .replace(/(?:\/Users|\/tmp|\/var|\/private|\/opt|\/mnt|\/home)\/[^\s"',)]+/g, '[redacted-path]')
    .replace(/[A-Za-z]:\\[^\s"',)]+/g, '[redacted-path]')
    .replace(/https?:\/\/[^\s"',)]+/g, '[redacted-url]');
}

function rectsIntersect(a: Rect, b: Rect): boolean {
  return a.x <= b.x + b.width
    && a.x + a.width >= b.x
    && a.y <= b.y + b.height
    && a.y + a.height >= b.y;
}

function isImeEvent(nativeEvent?: { isComposing?: boolean; keyCode?: number; which?: number }) {
  return Boolean(
    nativeEvent?.isComposing === true || nativeEvent?.keyCode === 229 || nativeEvent?.which === 229,
  );
}
