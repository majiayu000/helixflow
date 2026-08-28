import type {
  WorkbenchLayoutDocument,
  WorkbenchPaneDefinition,
  WorkbenchZoneId,
} from './types';

export const WORKBENCH_PANE_DEFINITIONS: Record<string, WorkbenchPaneDefinition> = {
  chat: {
    id: 'chat',
    title: 'Chat',
    defaultContainerId: 'conversation',
    defaultZone: 'secondarySidebar',
    allowedZones: ['secondarySidebar', 'primarySidebar'],
    canCollapse: true,
    canHide: false,
    singleton: true,
  },
  canvas: {
    id: 'canvas',
    title: 'Canvas',
    defaultContainerId: 'editor',
    defaultZone: 'editor',
    allowedZones: ['editor'],
    canCollapse: false,
    canHide: false,
    singleton: true,
  },
  artifact: {
    id: 'artifact',
    title: 'Artifact Viewer',
    defaultContainerId: 'artifactViewer',
    defaultZone: 'secondarySidebar',
    allowedZones: ['secondarySidebar', 'primarySidebar', 'panel'],
    canCollapse: true,
    canHide: true,
    singleton: true,
  },
  run: {
    id: 'run',
    title: 'Run',
    defaultContainerId: 'runMonitor',
    defaultZone: 'panel',
    allowedZones: ['panel', 'secondarySidebar', 'primarySidebar'],
    canCollapse: true,
    canHide: false,
    singleton: true,
  },
  outputs: {
    id: 'outputs',
    title: 'Outputs',
    defaultContainerId: 'outputs',
    defaultZone: 'panel',
    allowedZones: ['panel', 'secondarySidebar', 'primarySidebar'],
    canCollapse: true,
    canHide: true,
    singleton: true,
  },
};

export const WORKBENCH_ZONE_CONSTRAINTS: Record<
  Exclude<WorkbenchZoneId, 'editor'>,
  { defaultSizePx: number; minSizePx: number; maxSizePx: number }
> = {
  primarySidebar: { defaultSizePx: 420, minSizePx: 280, maxSizePx: 580 },
  secondarySidebar: { defaultSizePx: 480, minSizePx: 360, maxSizePx: 720 },
  panel: { defaultSizePx: 240, minSizePx: 120, maxSizePx: 520 },
};

export function createDefaultWorkbenchLayout(): WorkbenchLayoutDocument {
  return {
    schemaVersion: 3,
    profileId: 'default',
    zones: {
      primarySidebar: {
        visible: false,
        sizePx: WORKBENCH_ZONE_CONSTRAINTS.primarySidebar.defaultSizePx,
        containerIds: [],
        activeContainerId: null,
      },
      editor: {
        visible: true,
        sizePx: null,
        containerIds: ['editor'],
        activeContainerId: 'editor',
      },
      secondarySidebar: {
        visible: true,
        sizePx: WORKBENCH_ZONE_CONSTRAINTS.secondarySidebar.defaultSizePx,
        containerIds: ['conversation', 'artifactViewer'],
        activeContainerId: 'conversation',
      },
      panel: {
        visible: false,
        sizePx: WORKBENCH_ZONE_CONSTRAINTS.panel.defaultSizePx,
        containerIds: ['runMonitor', 'outputs'],
        activeContainerId: 'runMonitor',
      },
    },
    containers: {
      conversation: {
        id: 'conversation',
        zoneId: 'secondarySidebar',
        paneIds: ['chat'],
        activePaneId: 'chat',
      },
      editor: {
        id: 'editor',
        zoneId: 'editor',
        paneIds: ['canvas'],
        activePaneId: 'canvas',
      },
      artifactViewer: {
        id: 'artifactViewer',
        zoneId: 'secondarySidebar',
        paneIds: ['artifact'],
        activePaneId: 'artifact',
      },
      runMonitor: {
        id: 'runMonitor',
        zoneId: 'panel',
        paneIds: ['run'],
        activePaneId: 'run',
      },
      outputs: {
        id: 'outputs',
        zoneId: 'panel',
        paneIds: ['outputs'],
        activePaneId: 'outputs',
      },
    },
    panes: {
      chat: { id: 'chat', containerId: 'conversation', collapsed: false, visible: true },
      canvas: { id: 'canvas', containerId: 'editor', collapsed: false, visible: true },
      artifact: { id: 'artifact', containerId: 'artifactViewer', collapsed: false, visible: true },
      run: { id: 'run', containerId: 'runMonitor', collapsed: false, visible: true },
      outputs: { id: 'outputs', containerId: 'outputs', collapsed: false, visible: true },
    },
  };
}

export function clampWorkbenchZoneSize(
  zoneId: Exclude<WorkbenchZoneId, 'editor'>,
  sizePx: number,
): number {
  const constraint = WORKBENCH_ZONE_CONSTRAINTS[zoneId];
  return Math.min(constraint.maxSizePx, Math.max(constraint.minSizePx, Math.round(sizePx)));
}
