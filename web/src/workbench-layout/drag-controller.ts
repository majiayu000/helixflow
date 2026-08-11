import type { WorkbenchDropTarget } from './types';

export type WorkbenchDragSession = {
  containerId: string;
  pointerId: number;
  startX: number;
  startY: number;
  active: boolean;
  target: WorkbenchDropTarget | null;
};

export function beginWorkbenchDrag(
  containerId: string,
  pointerId: number,
  startX: number,
  startY: number,
): WorkbenchDragSession {
  return { containerId, pointerId, startX, startY, active: false, target: null };
}

export function updateWorkbenchDrag(
  session: WorkbenchDragSession,
  pointerId: number,
  x: number,
  y: number,
  target: WorkbenchDropTarget | null,
  thresholdPx = 6,
): WorkbenchDragSession {
  if (pointerId !== session.pointerId) return session;
  const active = session.active || Math.hypot(x - session.startX, y - session.startY) >= thresholdPx;
  return { ...session, active, target: active ? target : null };
}

export function workbenchDragCommit(
  session: WorkbenchDragSession | null,
  pointerId: number,
): { containerId: string; target: WorkbenchDropTarget } | null {
  if (!session || session.pointerId !== pointerId || !session.active || !session.target) return null;
  return { containerId: session.containerId, target: session.target };
}
