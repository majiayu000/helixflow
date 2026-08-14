import { clampWorkbenchZoneSize } from './defaults';
import type { WorkbenchZoneId } from './types';

export type ResizableWorkbenchZoneId = Exclude<WorkbenchZoneId, 'editor'>;

export type WorkbenchResizeSession = {
  zoneId: ResizableWorkbenchZoneId;
  pointerId: number;
  startX: number;
  startY: number;
  startSizePx: number;
};

export function beginWorkbenchResize(
  zoneId: ResizableWorkbenchZoneId,
  pointerId: number,
  startX: number,
  startY: number,
  startSizePx: number,
): WorkbenchResizeSession {
  return { zoneId, pointerId, startX, startY, startSizePx };
}

export function workbenchResizeSize(
  session: WorkbenchResizeSession,
  pointerId: number,
  x: number,
  y: number,
): number | null {
  if (session.pointerId !== pointerId) return null;
  let delta = x - session.startX;
  if (session.zoneId === 'secondarySidebar') delta = session.startX - x;
  if (session.zoneId === 'panel') delta = session.startY - y;
  return clampWorkbenchZoneSize(session.zoneId, session.startSizePx + delta);
}
