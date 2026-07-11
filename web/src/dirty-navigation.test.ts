import { describe, expect, it, vi } from 'vitest';
import {
  pendingNavigationLabel,
  planNavigationRequest,
  resolveDirtyNavigation,
} from './dirty-navigation';

describe('dirty navigation coordinator', () => {
  it('cancels without mutating or navigating', async () => {
    const actions = navigationActions();

    await expect(resolveDirtyNavigation('cancel', actions)).resolves.toBe(false);

    expect(actions.commit).not.toHaveBeenCalled();
    expect(actions.discard).not.toHaveBeenCalled();
    expect(actions.navigate).not.toHaveBeenCalled();
  });

  it('discards before navigating', async () => {
    const order: string[] = [];
    const actions = navigationActions({
      discard: vi.fn(() => order.push('discard')),
      navigate: vi.fn(async () => { order.push('navigate'); }),
    });

    await expect(resolveDirtyNavigation('discard', actions)).resolves.toBe(true);

    expect(order).toEqual(['discard', 'navigate']);
  });

  it('commits and verifies the edit session cleared before navigating', async () => {
    let dirty = true;
    const order: string[] = [];
    const actions = navigationActions({
      commit: vi.fn(async () => { dirty = false; order.push('commit'); }),
      isDirty: vi.fn(() => dirty),
      navigate: vi.fn(async () => { order.push('navigate'); }),
    });

    await expect(resolveDirtyNavigation('commit', actions)).resolves.toBe(true);

    expect(order).toEqual(['commit', 'navigate']);
  });

  it('does not navigate when commit fails or leaves dirty edits', async () => {
    const rejected = navigationActions({ commit: vi.fn(async () => { throw new Error('save failed'); }) });
    await expect(resolveDirtyNavigation('commit', rejected)).rejects.toThrow('save failed');
    expect(rejected.navigate).not.toHaveBeenCalled();

    const stillDirty = navigationActions({ isDirty: vi.fn(() => true) });
    await expect(resolveDirtyNavigation('commit', stillDirty)).rejects.toThrow(
      'manual edits remain after commit',
    );
    expect(stillDirty.navigate).not.toHaveBeenCalled();
  });

  it('labels every guarded navigation target', () => {
    expect(pendingNavigationLabel({ kind: 'workspace', workspaceId: 'ws_b' })).toContain('ws_b');
    expect(pendingNavigationLabel({ kind: 'restore', versionId: 'ver_1' })).toContain('ver_1');
    expect(pendingNavigationLabel({ kind: 'create_workspace' })).toContain('新建');
    expect(pendingNavigationLabel({ kind: 'undo' })).toContain('撤销');
  });

  it('routes clean, dirty, current, and pending navigation requests', () => {
    expect(planNavigationRequest({ hasDirtyEdits: false, isCurrentWorkspace: false, locked: false }))
      .toBe('navigate');
    expect(planNavigationRequest({ hasDirtyEdits: true, isCurrentWorkspace: false, locked: false }))
      .toBe('prompt');
    expect(planNavigationRequest({ hasDirtyEdits: true, isCurrentWorkspace: true, locked: false }))
      .toBe('ignore');
    expect(planNavigationRequest({ hasDirtyEdits: false, isCurrentWorkspace: false, locked: true }))
      .toBe('ignore');
  });
});

function navigationActions(overrides: Partial<Parameters<typeof resolveDirtyNavigation>[1]> = {}) {
  return {
    commit: vi.fn(async () => undefined),
    discard: vi.fn(),
    isDirty: vi.fn(() => false),
    navigate: vi.fn(async () => undefined),
    ...overrides,
  };
}
