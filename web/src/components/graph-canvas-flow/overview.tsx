import { useEffect, useMemo, useRef } from 'react';
import type { GraphNodeState } from '../../types';
import {
  graphNodeHeight,
  graphNodeWidth,
  type ViewState,
} from '../graph-canvas-navigation';

const OVERVIEW_NODE_THRESHOLD = 2_000;
const OVERVIEW_ZOOM_THRESHOLD = 0.075;
const MAX_BITMAP_DIMENSION = 2_048;

export function shouldUseCanvasOverview(nodeCount: number, view: ViewState): boolean {
  return nodeCount > OVERVIEW_NODE_THRESHOLD && view.z < OVERVIEW_ZOOM_THRESHOLD;
}

/**
 * Low-zoom dense-graph LOD. React Flow remains responsible for the viewport,
 * while one transformed bitmap replaces thousands of detailed DOM nodes.
 */
export function CanvasOverview({ nodes }: { nodes: GraphNodeState[] }) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const layout = useMemo(() => overviewLayout(nodes), [nodes]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !layout) return;
    canvas.width = layout.bitmapWidth;
    canvas.height = layout.bitmapHeight;
    const context = canvas.getContext('2d');
    if (!context) return;
    context.clearRect(0, 0, canvas.width, canvas.height);
    context.scale(layout.scale, layout.scale);
    for (const node of layout.nodes) {
      context.fillStyle = overviewColor(node.category);
      context.fillRect(
        node.position.x - layout.left,
        node.position.y - layout.top,
        graphNodeWidth(node),
        graphNodeHeight(node),
      );
    }
  }, [layout]);

  if (!layout) return null;
  return (
    <canvas
      ref={canvasRef}
      className="flow-canvas-overview"
      aria-label={`${nodes.length.toLocaleString()} 节点画布概览`}
      role="img"
      style={{
        height: layout.height,
        left: layout.left,
        top: layout.top,
        width: layout.width,
      }}
    />
  );
}

function overviewLayout(nodes: GraphNodeState[]) {
  const measured = nodes.filter((node) => (
    Number.isFinite(node.position.x) && Number.isFinite(node.position.y)
  ));
  if (measured.length === 0) return null;
  let left = Infinity;
  let top = Infinity;
  let right = -Infinity;
  let bottom = -Infinity;
  for (const node of measured) {
    left = Math.min(left, node.position.x);
    top = Math.min(top, node.position.y);
    right = Math.max(right, node.position.x + graphNodeWidth(node));
    bottom = Math.max(bottom, node.position.y + graphNodeHeight(node));
  }
  const width = Math.max(1, right - left);
  const height = Math.max(1, bottom - top);
  const scale = Math.min(1, MAX_BITMAP_DIMENSION / width, MAX_BITMAP_DIMENSION / height);
  return {
    bitmapHeight: Math.max(1, Math.ceil(height * scale)),
    bitmapWidth: Math.max(1, Math.ceil(width * scale)),
    height,
    left,
    nodes: measured,
    scale,
    top,
    width,
  };
}

function overviewColor(category: string): string {
  const key = category.toLowerCase();
  if (key.includes('video')) return '#f3a35c';
  if (key.includes('image') || key.includes('input')) return '#58c7d9';
  if (key.includes('output')) return '#70d6a3';
  return '#b89cff';
}
