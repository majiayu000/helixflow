export type CanvasMode = 'view' | 'edit' | 'review';

export type CanvasCapabilities = {
  select: true;
  pan: true;
  zoom: true;
  move: boolean;
  resize: boolean;
  connect: boolean;
  delete: boolean;
  paste: boolean;
};

export function canvasCapabilities(
  mode: CanvasMode,
  hasMutationHandler: boolean,
): CanvasCapabilities {
  const canMutate = mode === 'edit' && hasMutationHandler;
  return {
    select: true,
    pan: true,
    zoom: true,
    move: canMutate,
    resize: canMutate,
    connect: canMutate,
    delete: canMutate,
    paste: canMutate,
  };
}
