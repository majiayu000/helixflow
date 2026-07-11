export type WorkspaceActivation = {
  generation: number;
  signal: AbortSignal;
};

export class WorkspaceRequestScope {
  private generation = 0;
  private workspaceId: string | null = null;
  private controller: AbortController | null = null;

  begin(workspaceId: string | null): WorkspaceActivation {
    this.generation += 1;
    this.workspaceId = workspaceId;
    this.controller?.abort();
    this.controller = new AbortController();
    return { generation: this.generation, signal: this.controller.signal };
  }

  adopt(generation: number, workspaceId: string): boolean {
    if (generation !== this.generation) return false;
    this.workspaceId = workspaceId;
    return true;
  }

  isActive(generation: number, workspaceId?: string): boolean {
    return (
      generation === this.generation &&
      (workspaceId === undefined || this.workspaceId === workspaceId)
    );
  }

  currentGeneration(): number {
    return this.generation;
  }

  currentWorkspaceId(): string | null {
    return this.workspaceId;
  }

  signal(): AbortSignal | undefined {
    return this.controller?.signal;
  }
}

export function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError';
}
