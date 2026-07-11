import { describe, expect, it } from 'vitest';
import { applyRunEvent, shouldRefetchWorkspaceState } from './store-events';
import type { WorkbenchState } from './types';

const state = {
  workspace: { id: 'ws_1' },
  run: { id: 'run_parent' },
  eventSeq: 4,
  chat: { messages: [] },
} as unknown as WorkbenchState;

describe('run retry events', () => {
  it('adds a visible retry notice and requests the child snapshot', () => {
    const event = {
      workspace_id: 'ws_1',
      run_id: 'run_parent',
      seq: 5,
      server_time: '2026-07-11T00:00:00Z',
      ev: 'run.retry',
      data: {
        child_run_id: 'run_child',
        attempt: 1,
        requires_confirmation: true,
      },
    };

    const updated = applyRunEvent(state, event);
    expect(updated.chat.messages.at(-1)?.text).toContain('waiting for cost confirmation');
    expect(shouldRefetchWorkspaceState(state, event)).toBe(true);
  });

  it('refetches when a child retry announces pending confirmation', () => {
    expect(
      shouldRefetchWorkspaceState(state, {
        workspace_id: 'ws_1',
        run_id: 'run_child',
        seq: 1,
        server_time: '2026-07-11T00:00:01Z',
        ev: 'run.retry_pending',
        data: { parent_run_id: 'run_parent' },
      }),
    ).toBe(true);
  });
});
