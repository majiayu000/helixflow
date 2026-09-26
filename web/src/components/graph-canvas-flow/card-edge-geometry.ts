export type CardBox = { x: number; y: number; width: number; height: number };

export function mediaCardAnchor(box: CardBox, side: 'left' | 'right'): { x: number; y: number } {
  return {
    x: side === 'right' ? box.x + box.width : box.x,
    y: box.y + box.height / 2,
  };
}

export function internalNodeBox(node: {
  internals: { positionAbsolute: { x: number; y: number } };
  measured?: { width?: number; height?: number };
  width?: number;
  height?: number;
}): CardBox | null {
  const width = node.measured?.width ?? node.width ?? 0;
  const height = node.measured?.height ?? node.height ?? 0;
  if (!(width > 0 && height > 0)) return null;
  return {
    x: node.internals.positionAbsolute.x,
    y: node.internals.positionAbsolute.y,
    width,
    height,
  };
}
