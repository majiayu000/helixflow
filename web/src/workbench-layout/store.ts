import { create } from 'zustand';
import { reduceWorkbenchLayout } from './reducer';
import {
  browserWorkbenchLayoutStorage,
  loadWorkbenchLayout,
  saveWorkbenchLayout,
  type WorkbenchLayoutStorage,
} from './storage';
import type {
  WorkbenchLayoutCommand,
  WorkbenchLayoutDiagnostic,
  WorkbenchLayoutDocument,
  WorkbenchLayoutResult,
} from './types';

type WorkbenchLayoutStore = {
  document: WorkbenchLayoutDocument;
  diagnostics: WorkbenchLayoutDiagnostic[];
  dispatch: (command: WorkbenchLayoutCommand) => WorkbenchLayoutResult;
  hydrate: (storage?: WorkbenchLayoutStorage | null) => void;
};

const initialStorage = browserWorkbenchLayoutStorage();
const initialLayout = loadWorkbenchLayout(initialStorage);
let activeStorage = initialStorage;

export const useWorkbenchLayoutStore = create<WorkbenchLayoutStore>((set, get) => ({
  document: initialLayout.layout,
  diagnostics: initialLayout.diagnostic ? [initialLayout.diagnostic] : [],
  dispatch: (command) => {
    const result = reduceWorkbenchLayout(get().document, command);
    if (!result.ok) {
      set((current) => ({
        diagnostics: [...current.diagnostics, { kind: 'command', message: result.error.message }],
      }));
      return result;
    }
    const diagnostic = saveWorkbenchLayout(activeStorage, result.state);
    set((current) => ({
      document: result.state,
      diagnostics: diagnostic ? [...current.diagnostics, diagnostic] : current.diagnostics,
    }));
    return result;
  },
  hydrate: (storage = browserWorkbenchLayoutStorage()) => {
    activeStorage = storage;
    const loaded = loadWorkbenchLayout(storage);
    set({
      document: loaded.layout,
      diagnostics: loaded.diagnostic ? [loaded.diagnostic] : [],
    });
  },
}));

export function resetWorkbenchLayoutStoreForTests(storage: WorkbenchLayoutStorage | null = null) {
  useWorkbenchLayoutStore.getState().hydrate(storage);
}
