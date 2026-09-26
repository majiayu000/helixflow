import type { GraphNodeState } from '../types';
import { graphNodeHeight, graphNodeWidth } from './graph-canvas-navigation';

export type CanvasGroup = {
  id: string;
  memberIds: string[];
};

export type GroupFrame = {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
};

const STORAGE_PREFIX = 'helixflow:canvas-groups:v1:';
const FRAME_PAD = 18;
const FRAME_TOP = 28;

type GroupStorage = Pick<Storage, 'getItem' | 'setItem'>;

export function loadCanvasGroups(workspaceId: string): CanvasGroup[] {
  const storage = safeLocalStorage();
  if (!storage || !workspaceId) return [];
  try {
    const parsed = JSON.parse(storage.getItem(`${STORAGE_PREFIX}${workspaceId}`) ?? '[]');
    if (!Array.isArray(parsed)) return [];
    return parsed.flatMap((item) => {
      if (!item || typeof item !== 'object') return [];
      const record = item as { id?: unknown; memberIds?: unknown };
      if (typeof record.id !== 'string' || !Array.isArray(record.memberIds)) return [];
      const memberIds = record.memberIds.filter((id): id is string => typeof id === 'string');
      return memberIds.length >= 2 ? [{ id: record.id, memberIds }] : [];
    });
  } catch {
    return [];
  }
}

export function saveCanvasGroups(workspaceId: string, groups: CanvasGroup[]): void {
  const storage = safeLocalStorage();
  if (!storage || !workspaceId) return;
  try {
    storage.setItem(`${STORAGE_PREFIX}${workspaceId}`, JSON.stringify(groups));
  } catch {
    // Persistence is best-effort.
  }
}

export function canGroupSelectedNodes(selectedIds: ReadonlySet<string>): boolean {
  return selectedIds.size >= 2;
}

export function canUngroupSelectedNodes(
  selectedIds: ReadonlySet<string>,
  groups: CanvasGroup[],
): boolean {
  return groups.some((group) => group.memberIds.some((id) => selectedIds.has(id)));
}

export function applyGroupSelection(selectedIds: ReadonlySet<string>, groups: CanvasGroup[]): CanvasGroup[] {
  const memberIds = [...selectedIds];
  if (memberIds.length < 2) return groups;
  const covered = new Set(memberIds);
  const remaining = groups.filter((group) => !group.memberIds.every((id) => covered.has(id)));
  return [...remaining, { id: `group_${cryptoRandom()}`, memberIds }];
}

export function applyUngroupSelection(selectedIds: ReadonlySet<string>, groups: CanvasGroup[]): CanvasGroup[] {
  return groups.filter((group) => !group.memberIds.some((id) => selectedIds.has(id)));
}

export function expandSelectionWithGroups(
  selectedIds: ReadonlySet<string>,
  groups: CanvasGroup[],
): Set<string> {
  const next = new Set(selectedIds);
  for (const group of groups) {
    if (group.memberIds.some((id) => next.has(id))) {
      for (const id of group.memberIds) next.add(id);
    }
  }
  return next;
}

export function groupFrames(groups: CanvasGroup[], nodes: GraphNodeState[]): GroupFrame[] {
  const byId = new Map(nodes.map((node) => [node.id, node] as const));
  return groups.flatMap((group) => {
    const members = group.memberIds.map((id) => byId.get(id)).filter((node): node is GraphNodeState => Boolean(node));
    if (members.length < 2) return [];
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const node of members) {
      minX = Math.min(minX, node.position.x);
      minY = Math.min(minY, node.position.y);
      maxX = Math.max(maxX, node.position.x + graphNodeWidth(node));
      maxY = Math.max(maxY, node.position.y + graphNodeHeight(node));
    }
    return [{
      id: group.id,
      x: minX - FRAME_PAD,
      y: minY - FRAME_TOP,
      width: maxX - minX + FRAME_PAD * 2,
      height: maxY - minY + FRAME_PAD + FRAME_TOP,
    }];
  });
}

function cryptoRandom(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID().slice(0, 8);
  }
  return Math.random().toString(36).slice(2, 10);
}

function safeLocalStorage(): GroupStorage | null {
  try {
    const storage = globalThis.localStorage as Partial<Storage> | undefined;
    return storage && typeof storage.getItem === 'function' && typeof storage.setItem === 'function'
      ? (storage as GroupStorage)
      : null;
  } catch {
    return null;
  }
}
