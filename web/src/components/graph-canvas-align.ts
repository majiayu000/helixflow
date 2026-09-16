import type { GraphNodeState, LayoutPositionUpdate } from '../types';
import { graphNodeHeight, graphNodeWidth } from './graph-canvas-navigation';

export type AlignKind =
  | 'left'
  | 'center'
  | 'right'
  | 'top'
  | 'middle'
  | 'bottom'
  | 'distribute-x'
  | 'distribute-y';

type Box = {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
};

export function alignNodePositions(nodes: GraphNodeState[], kind: AlignKind): LayoutPositionUpdate[] {
  if (nodes.length < 2) return [];
  const boxes = nodes.map(toBox);
  const next = boxes.map((box) => ({ id: box.id, x: box.x, y: box.y }));
  if (kind === 'left') {
    const left = Math.min(...boxes.map((box) => box.x));
    next.forEach((item, index) => { item.x = left; void index; });
  } else if (kind === 'right') {
    const right = Math.max(...boxes.map((box) => box.x + box.width));
    next.forEach((item, index) => { item.x = right - boxes[index]!.width; });
  } else if (kind === 'center') {
    const mid = (Math.min(...boxes.map((box) => box.x)) + Math.max(...boxes.map((box) => box.x + box.width))) / 2;
    next.forEach((item, index) => { item.x = mid - boxes[index]!.width / 2; });
  } else if (kind === 'top') {
    const top = Math.min(...boxes.map((box) => box.y));
    next.forEach((item) => { item.y = top; });
  } else if (kind === 'bottom') {
    const bottom = Math.max(...boxes.map((box) => box.y + box.height));
    next.forEach((item, index) => { item.y = bottom - boxes[index]!.height; });
  } else if (kind === 'middle') {
    const mid = (Math.min(...boxes.map((box) => box.y)) + Math.max(...boxes.map((box) => box.y + box.height))) / 2;
    next.forEach((item, index) => { item.y = mid - boxes[index]!.height / 2; });
  } else if (kind === 'distribute-x' && boxes.length >= 3) {
    const ordered = [...boxes].sort((left, right) => left.x - right.x);
    const start = ordered[0]!.x;
    const span = ordered[ordered.length - 1]!.x - start;
    const step = span / (ordered.length - 1);
    ordered.forEach((box, index) => {
      const target = next.find((item) => item.id === box.id);
      if (target) target.x = start + step * index;
    });
  } else if (kind === 'distribute-y' && boxes.length >= 3) {
    const ordered = [...boxes].sort((left, right) => left.y - right.y);
    const start = ordered[0]!.y;
    const span = ordered[ordered.length - 1]!.y - start;
    const step = span / (ordered.length - 1);
    ordered.forEach((box, index) => {
      const target = next.find((item) => item.id === box.id);
      if (target) target.y = start + step * index;
    });
  }
  return next.filter((item, index) => item.x !== boxes[index]!.x || item.y !== boxes[index]!.y);
}

function toBox(node: GraphNodeState): Box {
  return {
    id: node.id,
    x: node.position.x,
    y: node.position.y,
    width: graphNodeWidth(node),
    height: graphNodeHeight(node),
  };
}
