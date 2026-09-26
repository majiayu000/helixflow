import { z } from 'zod';
import { createDefaultWorkbenchLayout } from './defaults';
import { reduceWorkbenchLayout, validateWorkbenchLayout } from './reducer';
import type {
  WorkbenchLayoutDiagnostic,
  WorkbenchLayoutDocument,
  WorkbenchZoneId,
} from './types';

export const WORKBENCH_LAYOUT_STORAGE_KEY = 'helixflow.workbench.layout.v4.canvas';
export const LEGACY_DOCK_LAYOUT_STORAGE_KEY = 'helixflow.workbench.dock-layout.v1';

export interface WorkbenchLayoutStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem?(key: string): void;
}

const zoneIdSchema = z.enum(['primarySidebar', 'editor', 'secondarySidebar', 'panel']);
const zoneSchema = z.object({
  visible: z.boolean(),
  sizePx: z.number().finite().nullable(),
  containerIds: z.array(z.string()),
  activeContainerId: z.string().nullable(),
});
const containerSchema = z.object({
  id: z.string(),
  zoneId: zoneIdSchema,
  paneIds: z.array(z.string()),
  activePaneId: z.string().nullable(),
});
const paneSchema = z.object({
  id: z.string(),
  containerId: z.string(),
  collapsed: z.boolean(),
  visible: z.boolean(),
});
const layoutSchema = z.object({
  schemaVersion: z.literal(3),
  profileId: z.literal('default'),
  zones: z.object({
    primarySidebar: zoneSchema,
    editor: zoneSchema,
    secondarySidebar: zoneSchema,
    panel: zoneSchema,
  }),
  containers: z.record(z.string(), containerSchema),
  panes: z.record(z.string(), paneSchema),
});

const legacySchema = z.object({
  order: z.array(z.enum(['run', 'outputs'])).optional(),
  collapsed: z.object({ run: z.boolean().optional(), outputs: z.boolean().optional() }).optional(),
  position: z.object({
    run: z.enum(['left', 'right', 'bottom']).optional(),
    outputs: z.enum(['left', 'right', 'bottom']).optional(),
  }).optional(),
});

export type LoadedWorkbenchLayout = {
  layout: WorkbenchLayoutDocument;
  diagnostic: WorkbenchLayoutDiagnostic | null;
  migrated: boolean;
};

export function loadWorkbenchLayout(storage: WorkbenchLayoutStorage | null): LoadedWorkbenchLayout {
  const fallback = createDefaultWorkbenchLayout();
  if (!storage) return { layout: fallback, diagnostic: null, migrated: false };
  try {
    const serialized = storage.getItem(WORKBENCH_LAYOUT_STORAGE_KEY);
    if (serialized) {
      const parsed = layoutSchema.safeParse(JSON.parse(serialized));
      if (!parsed.success) {
        return invalidHydration(fallback, 'Stored workbench layout does not match schema v3.');
      }
      const layout = parsed.data as WorkbenchLayoutDocument;
      const invariantError = validateWorkbenchLayout(layout);
      if (invariantError) return invalidHydration(fallback, invariantError.message);
      return { layout, diagnostic: null, migrated: false };
    }

    const legacySerialized = storage.getItem(LEGACY_DOCK_LAYOUT_STORAGE_KEY);
    if (!legacySerialized) return { layout: fallback, diagnostic: null, migrated: false };
    const legacy = legacySchema.safeParse(JSON.parse(legacySerialized));
    if (!legacy.success) return invalidHydration(fallback, 'Legacy dock layout is invalid.');
    const migrated = migrateLegacyDockLayout(legacy.data);
    storage.setItem(WORKBENCH_LAYOUT_STORAGE_KEY, JSON.stringify(migrated));
    return { layout: migrated, diagnostic: null, migrated: true };
  } catch (cause) {
    return invalidHydration(fallback, `Workbench layout hydration failed: ${errorMessage(cause)}`);
  }
}

export function saveWorkbenchLayout(
  storage: WorkbenchLayoutStorage | null,
  layout: WorkbenchLayoutDocument,
): WorkbenchLayoutDiagnostic | null {
  if (!storage) return null;
  try {
    storage.setItem(WORKBENCH_LAYOUT_STORAGE_KEY, JSON.stringify(layout));
    return null;
  } catch (cause) {
    return { kind: 'persist', message: `Workbench layout persistence failed: ${errorMessage(cause)}` };
  }
}

export function browserWorkbenchLayoutStorage(): WorkbenchLayoutStorage | null {
  try {
    const storage = globalThis.localStorage as Partial<Storage> | undefined;
    if (typeof storage?.getItem !== 'function' || typeof storage.setItem !== 'function') return null;
    return {
      getItem: (key) => storage.getItem!(key),
      setItem: (key, value) => storage.setItem!(key, value),
      removeItem: typeof storage.removeItem === 'function' ? (key) => storage.removeItem!(key) : undefined,
    };
  } catch {
    return null;
  }
}

function migrateLegacyDockLayout(legacy: z.infer<typeof legacySchema>): WorkbenchLayoutDocument {
  let layout = createDefaultWorkbenchLayout();
  const order = legacy.order ?? ['run', 'outputs'];
  for (const id of order) {
    const containerId = id === 'run' ? 'runMonitor' : 'outputs';
    const zoneId = legacyPositionToZone(legacy.position?.[id]);
    const moved = reduceWorkbenchLayout(layout, { type: 'container/move', containerId, toZoneId: zoneId });
    if (moved.ok) layout = moved.state;
  }
  for (const id of ['run', 'outputs'] as const) {
    if (legacy.collapsed?.[id]) {
      const collapsed = reduceWorkbenchLayout(layout, { type: 'pane/toggleCollapsed', paneId: id });
      if (collapsed.ok) layout = collapsed.state;
    }
  }
  return layout;
}

function legacyPositionToZone(value: 'left' | 'right' | 'bottom' | undefined): WorkbenchZoneId {
  if (value === 'left') return 'primarySidebar';
  if (value === 'right') return 'secondarySidebar';
  return 'panel';
}

function invalidHydration(
  layout: WorkbenchLayoutDocument,
  message: string,
): LoadedWorkbenchLayout {
  return { layout, diagnostic: { kind: 'hydrate', message }, migrated: false };
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
