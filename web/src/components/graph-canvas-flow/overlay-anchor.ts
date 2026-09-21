import type { CSSProperties } from 'react';
import type { WorkbenchState } from '../../types';
import { graphNodeHeight, graphNodeWidth } from '../graph-canvas-navigation';

type CanvasNode = WorkbenchState['graph']['nodes'][number];

export function flowCoverStyle(node: CanvasNode): CSSProperties {
  return {
    left: node.position.x,
    top: node.position.y,
    width: graphNodeWidth(node),
    height: graphNodeHeight(node),
  };
}

export function flowAboveCenterStyle(node: CanvasNode, gap = 10): CSSProperties {
  return {
    left: node.position.x + graphNodeWidth(node) / 2,
    top: node.position.y - gap,
  };
}

export function flowBelowStyle(node: CanvasNode, gap = 14): CSSProperties {
  return {
    left: node.position.x,
    top: node.position.y + graphNodeHeight(node) + gap,
    width: graphNodeWidth(node),
  };
}

export function flowBelowCenterStyle(node: CanvasNode, gap = 12): CSSProperties {
  return {
    left: node.position.x + graphNodeWidth(node) / 2,
    top: node.position.y + graphNodeHeight(node) + gap,
  };
}
