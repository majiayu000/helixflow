import type { ReactNode } from 'react';
import { WORKBENCH_PANE_DEFINITIONS } from './workbench-layout/defaults';

export type WorkbenchPaneBinding = {
  content: ReactNode;
  available: boolean;
  badge?: number | string;
};

export type WorkbenchPaneBindings = Record<string, WorkbenchPaneBinding>;

export const workbenchPaneRegistry = WORKBENCH_PANE_DEFINITIONS;
