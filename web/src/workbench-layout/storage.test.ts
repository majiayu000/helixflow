import { describe, expect, it } from 'vitest';
import { createDefaultWorkbenchLayout } from './defaults';
import {
  LEGACY_DOCK_LAYOUT_STORAGE_KEY,
  WORKBENCH_LAYOUT_STORAGE_KEY,
  loadWorkbenchLayout,
  saveWorkbenchLayout,
  type WorkbenchLayoutStorage,
} from './storage';

describe('workbench layout storage', () => {
  it('migrates v1 run and output preferences to independent containers', () => {
    const storage = memoryStorage({
      [LEGACY_DOCK_LAYOUT_STORAGE_KEY]: JSON.stringify({
        order: ['outputs', 'run'],
        collapsed: { run: true, outputs: false },
        position: { run: 'right', outputs: 'left' },
      }),
    });
    const loaded = loadWorkbenchLayout(storage);

    expect(loaded.migrated).toBe(true);
    expect(loaded.layout.containers.runMonitor.zoneId).toBe('secondarySidebar');
    expect(loaded.layout.containers.outputs.zoneId).toBe('primarySidebar');
    expect(loaded.layout.panes.run.collapsed).toBe(true);
    expect(storage.getItem(WORKBENCH_LAYOUT_STORAGE_KEY)).not.toBeNull();
  });

  it('falls back with a diagnostic for corrupt v2 storage', () => {
    const loaded = loadWorkbenchLayout(memoryStorage({ [WORKBENCH_LAYOUT_STORAGE_KEY]: '{nope' }));
    expect(loaded.layout).toEqual(createDefaultWorkbenchLayout());
    expect(loaded.diagnostic?.kind).toBe('hydrate');
  });

  it('keeps interaction in memory when persistence fails', () => {
    const diagnostic = saveWorkbenchLayout({
      getItem: () => null,
      setItem: () => { throw new Error('quota'); },
    }, createDefaultWorkbenchLayout());
    expect(diagnostic).toEqual({ kind: 'persist', message: 'Workbench layout persistence failed: quota' });
  });
});

function memoryStorage(seed: Record<string, string>): WorkbenchLayoutStorage {
  const values = new Map(Object.entries(seed));
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => { values.set(key, value); },
    removeItem: (key) => { values.delete(key); },
  };
}
