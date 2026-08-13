export const CANVAS_PRESENCE_INTERVAL_MS = 80;
export const CANVAS_PRESENCE_HEARTBEAT_MS = 5_000;
export const CANVAS_PRESENCE_TTL_MS = 15_000;

export const LOCAL_CANVAS_ACTOR = {
  actorId: createLocalCanvasActorId(),
  displayName: 'Local user',
};

export function isLocalCanvasActor(actorId: string): boolean {
  return actorId === LOCAL_CANVAS_ACTOR.actorId || actorId === 'local';
}

function createLocalCanvasActorId(): string {
  const randomId = globalThis.crypto?.randomUUID?.()
    ?? `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  return `local:${randomId}`;
}
