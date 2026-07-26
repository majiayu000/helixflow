import { describe, expect, it } from 'vitest';
import {
  applyRunEvent,
  preserveRetryNotices,
  shouldRefetchWorkspaceState,
} from './store-events';
import type { WorkbenchState } from './types';

const state = {
  workspace: { id: 'ws_1' },
  run: { id: 'run_parent' },
  eventSeq: 4,
  chat: { messages: [] },
} as unknown as WorkbenchState;

describe('remote cancel events', () => {
  it('shows a billing warning when the provider cannot cancel remote tasks', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'ws_1',
      run_id: 'run_parent',
      seq: 5,
      server_time: '2026-07-26T00:00:00Z',
      ev: 'run.remote_cancel_unsupported',
      data: {
        provider: 'atlas',
        provider_task_id: 'pred_1',
        message:
          'Provider `atlas` cannot cancel already-submitted remote tasks; the remote task may keep running and incur charges.',
      },
    });
    const notice = updated.chat.messages.at(-1);
    expect(notice?.role).toBe('system');
    expect(notice?.text).toContain('incur charges');
  });

  it('shows the error when remote cancellation fails', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'ws_1',
      run_id: 'run_parent',
      seq: 5,
      server_time: '2026-07-26T00:00:00Z',
      ev: 'run.remote_cancel_failed',
      data: { provider: 'fal', error: 'fal cancel returned HTTP 500' },
    });
    expect(updated.chat.messages.at(-1)?.text).toContain('fal cancel returned HTTP 500');
  });

  it('preserves remote cancel notices after a server snapshot refresh', () => {
    const withNotice = applyRunEvent(state, {
      workspace_id: 'ws_1',
      run_id: 'run_parent',
      seq: 6,
      server_time: '2026-07-26T00:00:01Z',
      ev: 'run.remote_cancel_unsupported',
      data: { provider: 'atlas', provider_task_id: 'pred_1' },
    });
    const snapshot = { ...state, chat: { messages: [] } } as unknown as WorkbenchState;

    const merged = preserveRetryNotices(withNotice, snapshot);
    expect(merged.chat.messages).toHaveLength(1);
    expect(merged.chat.messages[0]?.text).toContain('incur charges');
  });
});

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

  it('refetches when retry infrastructure fails visibly', () => {
    expect(
      shouldRefetchWorkspaceState(state, {
        workspace_id: 'ws_1',
        run_id: 'run_parent',
        seq: 6,
        server_time: '2026-07-11T00:00:02Z',
        ev: 'run.retry_failed',
        data: { error: 'retry persistence failed' },
      }),
    ).toBe(true);
  });

  it('preserves event-derived retry notices after a server snapshot refresh', () => {
    const withNotice = applyRunEvent(state, {
      workspace_id: 'ws_1',
      run_id: 'run_parent',
      seq: 7,
      server_time: '2026-07-11T00:00:03Z',
      ev: 'run.retry',
      data: { child_run_id: 'run_child', attempt: 1, requires_confirmation: false },
    });
    const snapshot = {
      ...state,
      run: { ...state.run, id: 'run_child' },
      chat: { messages: [] },
    } as unknown as WorkbenchState;

    const merged = preserveRetryNotices(withNotice, snapshot);
    expect(merged.chat.messages).toHaveLength(1);
    expect(merged.chat.messages[0]?.text).toContain('started automatically');
  });
});
