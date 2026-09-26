import { useCallback, useEffect, useRef, useState } from 'react';
import type { Viewport } from '@xyflow/react';
import type { GraphNodeState } from '../../types';
import {
  DEFAULT_GRAPH_VIEW,
  loadGraphCanvasView,
  saveGraphCanvasView,
  type ViewState,
  type ViewportSize,
} from '../graph-canvas-navigation';
import { fitViewToNodes } from '../graph-canvas-selection';
import type { WorkflowFlowInstance } from './types';

const VIEWPORT_SLICE_PAN_THRESHOLD = 240;
const VIEWPORT_SLICE_ZOOM_RATIO = 1.2;

export function useFlowViewport(
  workspaceId: string,
  persistedView?: ViewState | null,
  onViewCommit?: (view: ViewState) => void,
) {
  const canvasRef = useRef<HTMLElement | null>(null);
  const [instance, setInstance] = useState<WorkflowFlowInstance | null>(null);
  const initialView = persistedView ?? loadGraphCanvasView(workspaceId);
  const [view, setView] = useState<ViewState>(initialView);
  const [liveView, setLiveView] = useState<ViewState>(initialView);
  const [moving, setMoving] = useState(false);
  const [sliceView, setSliceView] = useState<ViewState>(initialView);
  const sliceViewRef = useRef(sliceView);
  const onViewCommitRef = useRef(onViewCommit);
  onViewCommitRef.current = onViewCommit;
  const [viewportSize, setViewportSize] = useState<ViewportSize>({ width: 900, height: 640 });

  useEffect(() => {
    const next = persistedView ?? loadGraphCanvasView(workspaceId);
    setView(next);
    setLiveView(next);
    sliceViewRef.current = next;
    setSliceView(next);
    if (instance) void instance.setViewport(toViewport(next));
  }, [instance, persistedView?.x, persistedView?.y, persistedView?.z, workspaceId]);

  useEffect(() => {
    const timer = setTimeout(() => saveGraphCanvasView(workspaceId, view), 180);
    return () => clearTimeout(timer);
  }, [view, workspaceId]);

  useEffect(() => {
    const current = canvasRef.current;
    if (!current) return;
    let frame: number | null = null;
    const updateSize = () => {
      const rect = current.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        setViewportSize((previous) => (
          previous.width === rect.width && previous.height === rect.height
            ? previous
            : { width: rect.width, height: rect.height }
        ));
      }
    };
    const scheduleUpdate = () => {
      if (frame !== null) cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        frame = null;
        updateSize();
      });
    };
    updateSize();
    const observer = typeof ResizeObserver === 'undefined'
      ? null
      : new ResizeObserver(scheduleUpdate);
    observer?.observe(current);
    return () => {
      observer?.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
    };
  }, []);

  const onInit = useCallback((next: WorkflowFlowInstance) => {
    setInstance(next);
    void next.setViewport(toViewport(loadGraphCanvasView(workspaceId)));
  }, [workspaceId]);

  const onMove = useCallback((_event: MouseEvent | TouchEvent | null, next: Viewport) => {
    const nextView = { x: next.x, y: next.y, z: next.zoom };
    setMoving(true);
    if (shouldRefreshViewportSlice(sliceViewRef.current, nextView)) {
      sliceViewRef.current = nextView;
      setSliceView(nextView);
    }
  }, []);

  const onMoveEnd = useCallback((_event: MouseEvent | TouchEvent | null, next: Viewport) => {
    const nextView = { x: next.x, y: next.y, z: next.zoom };
    setView(nextView);
    setLiveView(nextView);
    setMoving(false);
    sliceViewRef.current = nextView;
    setSliceView(nextView);
    onViewCommitRef.current?.(nextView);
  }, []);

  return { canvasRef, instance, liveView, moving, onInit, onMove, onMoveEnd, sliceView, view, viewportSize };
}

export async function setFlowViewportToNodes(
  instance: Pick<WorkflowFlowInstance, 'setViewport'> | null,
  nodes: GraphNodeState[],
  viewportSize: ViewportSize,
): Promise<void> {
  if (!instance || nodes.length === 0) return;
  const view = fitViewToNodes(nodes, viewportSize);
  await instance.setViewport(toViewport(view), { duration: 220 });
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
