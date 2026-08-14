import type { WorkbenchDropTarget, WorkbenchZoneId } from './types';

export function resolveWorkbenchDropTarget(
  root: HTMLElement,
  x: number,
  y: number,
  allowedZones: readonly WorkbenchZoneId[],
): WorkbenchDropTarget | null {
  const zones = root.querySelectorAll<HTMLElement>('[data-workbench-zone], [data-workbench-drop-zone]');
  for (const zone of zones) {
    const zoneId = (zone.dataset.workbenchDropZone ?? zone.dataset.workbenchZone) as WorkbenchZoneId | undefined;
    if (!zoneId || !allowedZones.includes(zoneId)) continue;
    const rect = zone.getBoundingClientRect();
    if (x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) {
      return { kind: 'zone', zoneId, index: Number.MAX_SAFE_INTEGER };
    }
  }
  return null;
}
