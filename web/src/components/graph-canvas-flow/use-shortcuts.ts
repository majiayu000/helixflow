import { useCallback, type KeyboardEvent } from 'react';
import type { GraphNodeState } from '../../types';
import type { createCanvasEditActions } from '../graph-canvas-edit-actions';
import { graphShortcutFromEvent, isEditableShortcutTarget } from '../graph-canvas-selection';
import type { WorkflowFlowInstance } from './types';

type ShortcutInput = {
  editActions: ReturnType<typeof createCanvasEditActions>;
  instance: WorkflowFlowInstance | null;
  nodes: GraphNodeState[];
  selectedIds: Set<string>;
  selectedNodes: GraphNodeState[];
  setSelection: (ids: Iterable<string>) => void;
};

export function useCanvasShortcuts(input: ShortcutInput) {
  return useCallback((event: KeyboardEvent<HTMLElement>) => {
    if (isEditableShortcutTarget(event.target)) return;
    const shortcut = graphShortcutFromEvent(event);
    if (!shortcut) return;
    event.preventDefault();
    if (shortcut === 'clear_selection') input.setSelection([]);
    if (shortcut === 'select_all') input.setSelection(input.nodes.map((node) => node.id));
    if (shortcut === 'fit_view') {
      void input.instance?.fitView({ duration: 220, padding: 0.18 });
    }
    if (shortcut === 'copy_selection') {
      void input.editActions.copySelection(input.selectedNodes);
    }
    if (shortcut === 'paste_selection') void input.editActions.pasteSelection();
    if (shortcut === 'delete_selection') input.editActions.deleteSelection(input.selectedIds);
  }, [input]);
}
