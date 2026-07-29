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

function recoveryEvent(seq: number, ev: string) {
  return {
    workspace_id: 'ws_1',
    run_id: 'run_parent',
    seq,
    server_time: '2026-07-30T00:00:00Z',
    ev,
    data: {},
  };
}

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

describe('run recovery events', () => {
  it('shows durable billing risk without exposing remote task identity', () => {
    const updated = applyRunEvent(state, {
      ...recoveryEvent(7, 'run.recovery_abandoned'),
      data: {
        provider: 'atlas',
        reason_code: 'missing_handle',
        message: '远端任务终态无法确认，可能继续产生费用。',
      },
    });

    const notice = updated.chat.messages.at(-1);
    expect(notice?.role).toBe('system');
    expect(notice?.text).toContain('可能继续产生费用');
    expect(notice?.text).not.toContain('task_');
    expect(shouldRefetchWorkspaceState(state, recoveryEvent(7, 'run.recovery_abandoned'))).toBe(true);
  });

  it('preserves recovery notices after a server snapshot refresh', () => {
    const withNotice = applyRunEvent(state, {
      ...recoveryEvent(8, 'run.recovery_started'),
      data: { reason_code: 'server_restart' },
    });
    const refreshed = preserveRetryNotices(withNotice, {
      ...state,
      chat: { messages: [] },
    } as unknown as WorkbenchState);

    expect(refreshed.chat.messages.some((message) => message.id.startsWith('run-recovery-'))).toBe(
      true,
    );
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

describe('run agent fix events', () => {
  it('distinguishes fix attempts from same-graph retries', () => {
    const event = {
      workspace_id: 'ws_1',
      run_id: 'run_parent',
      seq: 8,
      server_time: '2026-07-30T00:00:00Z',
      ev: 'run.fix_attempt',
      data: { operation_id: 'fix:root:1', attempt: 1, max_attempts: 2 },
    };
    const updated = applyRunEvent(state, event);
    expect(updated.chat.messages.at(-1)?.text).toContain('workflow repair attempt 1 of 2');
    expect(updated.chat.messages.at(-1)?.text).not.toContain('Retry');
  });

  it('shows a repaired version as pending execution rather than run success', () => {
    const event = {
      workspace_id: 'ws_1',
      run_id: 'run_parent',
      seq: 9,
      server_time: '2026-07-30T00:00:01Z',
      ev: 'run.fix_applied',
      data: {
        operation_id: 'fix:root:1',
        target_version_id: 'ver_fixed',
        child_run_id: 'run_fixed',
        requires_confirmation: true,
      },
    };
    const updated = applyRunEvent(state, event);
    expect(updated.chat.messages.at(-1)?.text).toContain('new version');
    expect(updated.chat.messages.at(-1)?.text).toContain('waiting for cost confirmation');
    expect(updated.chat.messages.at(-1)?.text).not.toContain('succeeded');
    expect(shouldRefetchWorkspaceState(state, event)).toBe(true);
  });

  it('preserves and deduplicates exhausted fix notices across snapshot refresh', () => {
    const event = {
      workspace_id: 'ws_1',
      run_id: 'run_parent',
      seq: 10,
      server_time: '2026-07-30T00:00:02Z',
      ev: 'run.fix_exhausted',
      data: { attempts: 2, max_attempts: 2, reason_code: 'FIX_LIMIT_REACHED' },
    };
    const withNotice = applyRunEvent(applyRunEvent(state, event), event);
    expect(withNotice.chat.messages).toHaveLength(1);
    const merged = preserveRetryNotices(withNotice, {
      ...state,
      chat: { messages: [] },
    } as unknown as WorkbenchState);
    expect(merged.chat.messages).toHaveLength(1);
    expect(merged.chat.messages[0]?.text).toContain('FIX_LIMIT_REACHED');
    expect(shouldRefetchWorkspaceState(state, event)).toBe(true);
  });
});
