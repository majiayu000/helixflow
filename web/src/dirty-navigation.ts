export type DirtyNavigationDecision = 'commit' | 'discard' | 'cancel';

export type PendingNavigation =
  | { kind: 'workspace'; workspaceId: string }
  | { kind: 'undo' }
  | { kind: 'restore'; versionId: string }
  | { kind: 'create_workspace' };

type DirtyNavigationActions = {
  commit: () => Promise<void>;
  discard: () => void;
  isDirty: () => boolean;
  navigate: () => Promise<void>;
};

export function planNavigationRequest(input: {
  hasDirtyEdits: boolean;
  isCurrentWorkspace: boolean;
  locked: boolean;
}): 'ignore' | 'navigate' | 'prompt' {
  if (input.locked || input.isCurrentWorkspace) return 'ignore';
  return input.hasDirtyEdits ? 'prompt' : 'navigate';
}

export async function resolveDirtyNavigation(
  decision: DirtyNavigationDecision,
  actions: DirtyNavigationActions,
): Promise<boolean> {
  if (decision === 'cancel') return false;
  if (decision === 'commit') {
    await actions.commit();
    if (actions.isDirty()) {
      throw new Error('manual edits remain after commit');
    }
  } else {
    actions.discard();
  }
  await actions.navigate();
  return true;
}

export function pendingNavigationLabel(target: PendingNavigation): string {
  if (target.kind === 'workspace') return `切换到 workspace ${target.workspaceId}`;
  if (target.kind === 'restore') return `恢复版本 ${target.versionId}`;
  if (target.kind === 'create_workspace') return '新建 workspace';
  return '撤销到上一版本';
}
