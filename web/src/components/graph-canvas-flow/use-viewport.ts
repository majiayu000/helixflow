import { useCallback, useEffect, useRef, useState } from 'react';
import type { Viewport } from '@xyflow/react';
import {
  DEFAULT_GRAPH_VIEW,
  loadGraphCanvasView,
  saveGraphCanvasView,
  type ViewState,
  type ViewportSize,
} from '../graph-canvas-navigation';
import type { WorkflowFlowInstance } from './types';

const VIEWPORT_SLICE_PAN_THRESHOLD = 240;
const VIEWPORT_SLICE_ZOOM_RATIO = 1.2;

export function useFlowViewport(workspaceId: string) {
  const canvasRef = useRef<HTMLElement | null>(null);
  const [instance, setInstance] = useState<WorkflowFlowInstance | null>(null);
  const [view, setView] = useState<ViewState>(() => loadGraphCanvasView(workspaceId));
  const [sliceView, setSliceView] = useState<ViewState>(() => loadGraphCanvasView(workspaceId));
  const sliceViewRef = useRef(sliceView);
  const [viewportSize, setViewportSize] = useState<ViewportSize>({ width: 900, height: 640 });

  useEffect(() => {
    const next = loadGraphCanvasView(workspaceId);
    setView(next);
    sliceViewRef.current = next;
    setSliceView(next);
    if (instance) void instance.setViewport(toViewport(next));
  }, [instance, workspaceId]);

  useEffect(() => {
    const timer = setTimeout(() => saveGraphCanvasView(workspaceId, view), 180);
    return () => clearTimeout(timer);
  }, [view, workspaceId]);

  useEffect(() => {
    const current = canvasRef.current;
    if (!current) return;
    const updateSize = () => {
      const rect = current.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        setViewportSize({ width: rect.width, height: rect.height });
      }
    };
    updateSize();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(updateSize);
    observer?.observe(current);
    return () => observer?.disconnect();
  }, []);

  const onInit = useCallback((next: WorkflowFlowInstance) => {
    setInstance(next);
    void next.setViewport(toViewport(loadGraphCanvasView(workspaceId)));
  }, [workspaceId]);

  const onMove = useCallback((_event: MouseEvent | TouchEvent | null, next: Viewport) => {
    const nextView = { x: next.x, y: next.y, z: next.zoom };
    if (shouldRefreshViewportSlice(sliceViewRef.current, nextView)) {
      sliceViewRef.current = nextView;
      setSliceView(nextView);
    }
  }, []);

  const onMoveEnd = useCallback((_event: MouseEvent | TouchEvent | null, next: Viewport) => {
    const nextView = { x: next.x, y: next.y, z: next.zoom };
    setView(nextView);
    sliceViewRef.current = nextView;
    setSliceView(nextView);
  }, []);

  return { canvasRef, instance, onInit, onMove, onMoveEnd, sliceView, view, viewportSize };
}

export function shouldRefreshViewportSlice(current: ViewState, next: ViewState): boolean {
  const zoomRatio = Math.max(next.z / current.z, current.z / next.z);
  return (
    Math.abs(next.x - current.x) > VIEWPORT_SLICE_PAN_THRESHOLD ||
    Math.abs(next.y - current.y) > VIEWPORT_SLICE_PAN_THRESHOLD ||
    zoomRatio > VIEWPORT_SLICE_ZOOM_RATIO
  );
}

function toViewport(view: ViewState): Viewport {
  return { x: view.x, y: view.y, zoom: view.z };
}

export { DEFAULT_GRAPH_VIEW };
