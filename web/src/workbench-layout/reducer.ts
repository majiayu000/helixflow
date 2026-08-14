import {
  WORKBENCH_PANE_DEFINITIONS,
  clampWorkbenchZoneSize,
  createDefaultWorkbenchLayout,
} from './defaults';
import type {
  WorkbenchLayoutCommand,
  WorkbenchLayoutDocument,
  WorkbenchLayoutError,
  WorkbenchLayoutResult,
  WorkbenchPaneDefinition,
  WorkbenchZoneId,
} from './types';

export function reduceWorkbenchLayout(
  current: WorkbenchLayoutDocument,
  command: WorkbenchLayoutCommand,
  definitions: Record<string, WorkbenchPaneDefinition> = WORKBENCH_PANE_DEFINITIONS,
): WorkbenchLayoutResult {
  if (command.type === 'layout/reset') {
    return { ok: true, state: createDefaultWorkbenchLayout() };
  }

  const next = cloneLayout(current);
  const failure = applyCommand(next, command, definitions);
  if (failure) return { ok: false, state: current, error: failure };
  const invariantError = validateWorkbenchLayout(next, definitions);
  if (invariantError) return { ok: false, state: current, error: invariantError };
  return { ok: true, state: next };
}

export function validateWorkbenchLayout(
  layout: WorkbenchLayoutDocument,
  definitions: Record<string, WorkbenchPaneDefinition> = WORKBENCH_PANE_DEFINITIONS,
): WorkbenchLayoutError | null {
  if (!layout.zones.editor.visible || layout.zones.editor.sizePx !== null) {
    return error('required_editor', 'Editor zone must remain visible and flexible.');
  }

  const seenContainers = new Set<string>();
  for (const [zoneId, zone] of Object.entries(layout.zones) as [WorkbenchZoneId, typeof layout.zones[WorkbenchZoneId]][]) {
    for (const containerId of zone.containerIds) {
      if (seenContainers.has(containerId)) {
        return error('invalid_layout', `Container ${containerId} appears in multiple zones.`);
      }
      const container = layout.containers[containerId];
      if (!container || container.zoneId !== zoneId) {
        return error('invalid_layout', `Container ${containerId} has an invalid zone reference.`);
      }
      seenContainers.add(containerId);
    }
  }

  if (seenContainers.size !== Object.keys(layout.containers).length) {
    return error('invalid_layout', 'Every container must belong to exactly one zone.');
  }

  const seenPanes = new Set<string>();
  for (const container of Object.values(layout.containers)) {
    for (const paneId of container.paneIds) {
      if (seenPanes.has(paneId)) {
        return error('invalid_layout', `Pane ${paneId} appears in multiple containers.`);
      }
      const pane = layout.panes[paneId];
      const definition = definitions[paneId];
      if (!pane || pane.containerId !== container.id || !definition) {
        return error('invalid_layout', `Pane ${paneId} has an invalid container or definition.`);
      }
      if (!definition.allowedZones.includes(container.zoneId)) {
        return error('forbidden_target', `Pane ${paneId} cannot be placed in ${container.zoneId}.`);
      }
      seenPanes.add(paneId);
    }
  }

  if (seenPanes.size !== Object.keys(layout.panes).length) {
    return error('invalid_layout', 'Every pane must belong to exactly one container.');
  }

  const canvas = layout.panes.canvas;
  if (!canvas?.visible || canvas.containerId !== 'editor') {
    return error('required_editor', 'Canvas pane must remain in the editor container.');
  }
  return null;
}

function applyCommand(
  layout: WorkbenchLayoutDocument,
  command: Exclude<WorkbenchLayoutCommand, { type: 'layout/reset' }>,
  definitions: Record<string, WorkbenchPaneDefinition>,
): WorkbenchLayoutError | null {
  if (command.type === 'zone/toggle') {
    if (command.zoneId === 'editor') return error('required_editor', 'Editor zone cannot be collapsed.');
    const zone = layout.zones[command.zoneId];
    if (!zone) return error('unknown_zone', `Unknown zone ${command.zoneId}.`);
    zone.visible = !zone.visible;
    return null;
  }

  if (command.type === 'zone/resize') {
    if (command.zoneId === 'editor') return error('required_editor', 'Editor zone cannot be resized directly.');
    if (!Number.isFinite(command.sizePx)) return error('invalid_size', 'Zone size must be finite.');
    layout.zones[command.zoneId].sizePx = clampWorkbenchZoneSize(command.zoneId, command.sizePx);
    return null;
  }

  if (command.type === 'container/move') {
    const container = layout.containers[command.containerId];
    if (!container) return error('unknown_container', `Unknown container ${command.containerId}.`);
    if (!layout.zones[command.toZoneId]) return error('unknown_zone', `Unknown zone ${command.toZoneId}.`);
    if (container.paneIds.some((paneId) => !definitions[paneId]?.allowedZones.includes(command.toZoneId))) {
      return error('forbidden_target', `Container ${command.containerId} cannot move to ${command.toZoneId}.`);
    }
    const from = layout.zones[container.zoneId];
    from.containerIds = from.containerIds.filter((id) => id !== container.id);
    const to = layout.zones[command.toZoneId];
    const index = clampIndex(command.index ?? to.containerIds.length, to.containerIds.length);
    to.containerIds.splice(index, 0, container.id);
    container.zoneId = command.toZoneId;
    to.visible = true;
    repairActiveContainer(from, layout);
    repairActiveContainer(to, layout);
    return null;
  }

  if (command.type === 'container/reorder') {
    const container = layout.containers[command.containerId];
    if (!container) return error('unknown_container', `Unknown container ${command.containerId}.`);
    const ids = layout.zones[container.zoneId].containerIds;
    const fromIndex = ids.indexOf(container.id);
    if (fromIndex < 0) return error('invalid_layout', `Container ${container.id} is detached.`);
    ids.splice(fromIndex, 1);
    ids.splice(clampIndex(command.index, ids.length), 0, container.id);
    return null;
  }

  if (command.type === 'pane/move') {
    const pane = layout.panes[command.paneId];
    const target = layout.containers[command.toContainerId];
    if (!pane) return error('unknown_pane', `Unknown pane ${command.paneId}.`);
    if (!target) return error('unknown_container', `Unknown container ${command.toContainerId}.`);
    if (!definitions[pane.id]?.allowedZones.includes(target.zoneId)) {
      return error('forbidden_target', `Pane ${pane.id} cannot move to ${target.zoneId}.`);
    }
    const source = layout.containers[pane.containerId];
    source.paneIds = source.paneIds.filter((id) => id !== pane.id);
    const index = clampIndex(command.index ?? target.paneIds.length, target.paneIds.length);
    target.paneIds.splice(index, 0, pane.id);
    pane.containerId = target.id;
    repairActivePane(source);
    repairActivePane(target);
    return null;
  }

  if (command.type === 'pane/reorder') {
    const pane = layout.panes[command.paneId];
    if (!pane) return error('unknown_pane', `Unknown pane ${command.paneId}.`);
    const ids = layout.containers[pane.containerId].paneIds;
    const fromIndex = ids.indexOf(pane.id);
    if (fromIndex < 0) return error('invalid_layout', `Pane ${pane.id} is detached.`);
    ids.splice(fromIndex, 1);
    ids.splice(clampIndex(command.index, ids.length), 0, pane.id);
    return null;
  }

  const pane = layout.panes[command.paneId];
  const definition = definitions[command.paneId];
  if (!pane || !definition) return error('unknown_pane', `Unknown pane ${command.paneId}.`);
  if (command.type === 'pane/toggleCollapsed') {
    if (!definition.canCollapse) return error('forbidden_target', `Pane ${pane.id} cannot collapse.`);
    pane.collapsed = !pane.collapsed;
  } else if (command.type === 'pane/activate') {
    layout.containers[pane.containerId].activePaneId = pane.id;
  } else if (command.type === 'pane/show') {
    pane.visible = true;
    layout.zones[layout.containers[pane.containerId].zoneId].visible = true;
  } else if (command.type === 'pane/hide') {
    if (!definition.canHide) return error('forbidden_target', `Pane ${pane.id} cannot hide.`);
    pane.visible = false;
  }
  return null;
}

function cloneLayout(layout: WorkbenchLayoutDocument): WorkbenchLayoutDocument {
  return {
    ...layout,
    zones: Object.fromEntries(Object.entries(layout.zones).map(([id, zone]) => [id, {
      ...zone,
      containerIds: [...zone.containerIds],
    }])) as WorkbenchLayoutDocument['zones'],
    containers: Object.fromEntries(Object.entries(layout.containers).map(([id, container]) => [id, {
      ...container,
      paneIds: [...container.paneIds],
    }])),
    panes: Object.fromEntries(Object.entries(layout.panes).map(([id, pane]) => [id, { ...pane }])),
  };
}

function repairActiveContainer(zone: WorkbenchLayoutDocument['zones'][WorkbenchZoneId], layout: WorkbenchLayoutDocument) {
  if (!zone.activeContainerId || !zone.containerIds.includes(zone.activeContainerId)) {
    zone.activeContainerId = zone.containerIds[0] ?? null;
  }
  if (zone.activeContainerId && !layout.containers[zone.activeContainerId]) {
    zone.activeContainerId = null;
  }
}

function repairActivePane(container: WorkbenchLayoutDocument['containers'][string]) {
  if (!container.activePaneId || !container.paneIds.includes(container.activePaneId)) {
    container.activePaneId = container.paneIds[0] ?? null;
  }
}

function clampIndex(index: number, length: number): number {
  return Math.max(0, Math.min(length, Math.trunc(index)));
}

function error(code: WorkbenchLayoutError['code'], message: string): WorkbenchLayoutError {
  return { code, message };
}
