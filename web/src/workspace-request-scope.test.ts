import { describe, expect, it } from 'vitest';
import { WorkspaceRequestScope } from './workspace-request-scope';

describe('WorkspaceRequestScope', () => {
  it('invalidates A after A to B and after A to B to A', () => {
    const scope = new WorkspaceRequestScope();
    const firstA = scope.begin('ws_a');
    const b = scope.begin('ws_b');
    const secondA = scope.begin('ws_a');

    expect(firstA.signal.aborted).toBe(true);
    expect(b.signal.aborted).toBe(true);
    expect(scope.isActive(firstA.generation, 'ws_a')).toBe(false);
    expect(scope.isActive(b.generation, 'ws_b')).toBe(false);
    expect(scope.isActive(secondA.generation, 'ws_a')).toBe(true);
  });

  it('adopts a created workspace only for the active generation', () => {
    const scope = new WorkspaceRequestScope();
    const create = scope.begin(null);
    scope.begin('ws_newer');

    expect(scope.adopt(create.generation, 'ws_created')).toBe(false);
    expect(scope.currentWorkspaceId()).toBe('ws_newer');
  });
});
