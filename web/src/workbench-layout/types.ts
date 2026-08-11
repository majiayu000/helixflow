export const WORKBENCH_ZONE_IDS = [
  'primarySidebar',
  'editor',
  'secondarySidebar',
  'panel',
] as const;

export type WorkbenchZoneId = (typeof WORKBENCH_ZONE_IDS)[number];

export type WorkbenchZoneState = {
  visible: boolean;
  sizePx: number | null;
  containerIds: string[];
  activeContainerId: string | null;
};

export type WorkbenchViewContainerState = {
  id: string;
  zoneId: WorkbenchZoneId;
  paneIds: string[];
  activePaneId: string | null;
};

export type WorkbenchPanePlacementState = {
  id: string;
  containerId: string;
  collapsed: boolean;
  visible: boolean;
};

export type WorkbenchLayoutDocument = {
  schemaVersion: 2;
  profileId: 'default';
  zones: Record<WorkbenchZoneId, WorkbenchZoneState>;
  containers: Record<string, WorkbenchViewContainerState>;
  panes: Record<string, WorkbenchPanePlacementState>;
};

export type WorkbenchPaneDefinition = {
  id: string;
  title: string;
  defaultContainerId: string;
  defaultZone: WorkbenchZoneId;
  allowedZones: readonly WorkbenchZoneId[];
  canCollapse: boolean;
  canHide: boolean;
  singleton: true;
};

export type WorkbenchLayoutCommand =
  | { type: 'zone/toggle'; zoneId: WorkbenchZoneId }
  | { type: 'zone/resize'; zoneId: WorkbenchZoneId; sizePx: number }
  | { type: 'container/move'; containerId: string; toZoneId: WorkbenchZoneId; index?: number }
  | { type: 'container/reorder'; containerId: string; index: number }
  | { type: 'pane/move'; paneId: string; toContainerId: string; index?: number }
  | { type: 'pane/reorder'; paneId: string; index: number }
  | { type: 'pane/toggleCollapsed'; paneId: string }
  | { type: 'pane/activate'; paneId: string }
  | { type: 'pane/show'; paneId: string }
  | { type: 'pane/hide'; paneId: string }
  | { type: 'layout/reset' };

export type WorkbenchLayoutErrorCode =
  | 'unknown_zone'
  | 'unknown_container'
  | 'unknown_pane'
  | 'forbidden_target'
  | 'required_editor'
  | 'invalid_layout'
  | 'invalid_size';

export type WorkbenchLayoutError = {
  code: WorkbenchLayoutErrorCode;
  message: string;
};

export type WorkbenchLayoutResult =
  | { ok: true; state: WorkbenchLayoutDocument }
  | { ok: false; state: WorkbenchLayoutDocument; error: WorkbenchLayoutError };

export type WorkbenchLayoutDiagnostic = {
  kind: 'hydrate' | 'persist' | 'command';
  message: string;
};

export type WorkbenchDropTarget = {
  kind: 'zone';
  zoneId: WorkbenchZoneId;
  index: number;
};
