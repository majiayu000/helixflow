import type { WorkspaceRequestScope } from './workspace-request-scope';

export class WorkspaceChangedError extends Error {}

export class WorkspaceActionGuard {
  constructor(private readonly requestScope: WorkspaceRequestScope) {}

  fail(message: string): never {
    throw new WorkspaceChangedError(message);
  }

  assertActive(workspaceId: string, label: string): void {
    const generation = this.requestScope.currentGeneration();
    if (!this.requestScope.isActive(generation, workspaceId)) {
      this.fail(`workspace changed before ${label}`);
    }
  }

  async run<T>(workspaceId: string, label: string, action: () => Promise<T>): Promise<T> {
    const generation = this.requestScope.currentGeneration();
    if (!this.requestScope.isActive(generation, workspaceId)) {
      this.fail(`workspace changed before ${label}`);
    }
    try {
      const result = await action();
      if (!this.requestScope.isActive(generation, workspaceId)) {
        this.fail(`workspace changed while ${label}`);
      }
      return result;
    } catch (error) {
      if (!this.requestScope.isActive(generation, workspaceId)) {
        this.fail(`workspace changed while ${label}`);
      }
      throw error;
    }
  }
}
