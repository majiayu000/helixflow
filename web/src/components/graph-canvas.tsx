import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent,
  type WheelEvent,
} from 'react';
import { Icon } from '../icons';
import type { GraphNodeState, LayoutPositionUpdate, WorkbenchState } from '../types';
import { GraphEdges } from './graph-canvas-edges';
import { GraphInspector } from './graph-canvas-inspector';
import {
  applyPositionDrafts,
  moveNodeDrafts,
  positionUpdatesFromDrafts,
  selectionForNodePointer,
  type DragNodeStart,
  type PositionDrafts,
} from './graph-canvas-layout';
import {
  DEFAULT_GRAPH_VIEW,
  MINIMAP_HEIGHT,
  MINIMAP_WIDTH,
  computeMinimapLayout,
  loadGraphCanvasView,
  normalizeView,
  saveGraphCanvasView,
  viewForMinimapPoint,
  zoomViewAtPoint,
  type ViewState,
  type ViewportSize,
} from './graph-canvas-navigation';
import { CanvasMinimap } from './graph-canvas-minimap';
import { WorkflowNode } from './graph-canvas-node';
import {
  buildComparableNodeMap,
  buildEdgeSignatureSet,
  buildNodeMap,
  buildRunStepStateMap,
  nodeDiffState,
  runStatusLabel,
} from './graph-canvas-rendering';

export {
  DEFAULT_GRAPH_VIEW,
  GRAPH_CANVAS_VIEW_STORAGE_PREFIX,
  clampZoom,
  computeMinimapLayout,
  loadGraphCanvasView,
  minimapViewportRect,
  normalizeView,
  saveGraphCanvasView,
  viewForMinimapPoint,
  viewStorageKey,
  zoomViewAtPoint,
} from './graph-canvas-navigation';
export type { MinimapLayout, ViewState, ViewportSize } from './graph-canvas-navigation';

type GraphCanvasProps = {
  workspaceId: string;
  versionId: string;
  graph: WorkbenchState['graph'];
  pendingProposal: WorkbenchState['pendingProposal'];
  run: NonNullable<WorkbenchState['run']>;
  onSaveLayout?: (positions: LayoutPositionUpdate[]) => Promise<void>;
};

type DragState = {
  pointerId: number;
  sx: number;
  sy: number;
  ox: number;
  oy: number;
};

type NodeDragState = {
  pointerId: number;
  sx: number;
  sy: number;
  starts: DragNodeStart[];
};

export function GraphCanvas({
  workspaceId,
  versionId,
  graph,
  pendingProposal,
  run,
  onSaveLayout,
}: GraphCanvasProps) {
  const canvasRef = useRef<HTMLElement | null>(null);
  const [view, setView] = useState<ViewState>(DEFAULT_GRAPH_VIEW);
  const [viewportSize, setViewportSize] = useState<ViewportSize>({ width: 900, height: 640 });
  const [mode, setMode] = useState<'view' | 'edit' | 'review'>('view');
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [draftPositions, setDraftPositions] = useState<PositionDrafts>({});
  const [layoutSaving, setLayoutSaving] = useState(false);
  const drag = useRef<DragState | null>(null);
  const nodeDrag = useRef<NodeDragState | null>(null);
  const drawGraph = pendingProposal?.previewGraph ?? graph;
  const activeMode = pendingProposal ? 'review' : mode;
  const displayNodes = useMemo(
    () => applyPositionDrafts(drawGraph.nodes, draftPositions),
    [drawGraph.nodes, draftPositions],
  );
  const baseComparableById = useMemo(() => buildComparableNodeMap(graph.nodes), [graph.nodes]);
  const nodeById = useMemo(() => buildNodeMap(displayNodes), [displayNodes]);
  const baseEdgeIds = useMemo(() => buildEdgeSignatureSet(graph.edges), [graph.edges]);
  const stepStateByNodeId = useMemo(() => buildRunStepStateMap(run.steps), [run.steps]);
  const selectedNodeId = selectedIds.values().next().value as string | undefined;
  const selectedNode = selectedNodeId ? nodeById.get(selectedNodeId) : null;
  const nodeCount = displayNodes.length;
  const layoutUpdates = useMemo(
    () => (pendingProposal ? [] : positionUpdatesFromDrafts(graph.nodes, draftPositions)),
    [draftPositions, graph.nodes, pendingProposal],
  );
  const hasDirtyLayout = layoutUpdates.length > 0;
  const minimapLayout = useMemo(
    () => computeMinimapLayout(displayNodes, { width: MINIMAP_WIDTH, height: MINIMAP_HEIGHT }),
    [displayNodes],
  );

  useEffect(() => {
    setView(loadGraphCanvasView(workspaceId));
    setSelectedIds(new Set());
  }, [workspaceId]);

  useEffect(() => {
    setDraftPositions({});
    setSelectedIds(new Set());
    nodeDrag.current = null;
  }, [workspaceId, versionId, pendingProposal?.id]);

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
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(updateSize);
    observer.observe(current);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const timer = setTimeout(() => saveGraphCanvasView(workspaceId, view), 180);
    return () => clearTimeout(timer);
  }, [view, workspaceId]);

  const updateView = useCallback((next: ViewState | ((current: ViewState) => ViewState)) => {
    setView((current) => normalizeView(typeof next === 'function' ? next(current) : next));
  }, []);

  const handleWheel = (event: WheelEvent<HTMLElement>) => {
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest('.canvas-toolbar,.zoom-ctl,.inspector,.canvas-minimap')) return;
    event.preventDefault();
    const rect = event.currentTarget.getBoundingClientRect();
    updateView((current) =>
      zoomViewAtPoint(current, {
        deltaY: event.deltaY,
        localX: event.clientX - rect.left,
        localY: event.clientY - rect.top,
      }),
    );
  };

  const updateViewFromMinimap = useCallback(
    (x: number, y: number) => {
      if (!minimapLayout) return;
      updateView((current) =>
        viewForMinimapPoint(minimapLayout, { x, y }, current, viewportSize),
      );
    },
    [minimapLayout, updateView, viewportSize],
  );

  const stopDrag = (event: PointerEvent<HTMLElement>) => {
    const currentDrag = drag.current;
    if (!currentDrag || currentDrag.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    drag.current = null;
  };

  const handleNodePointerDown = (node: GraphNodeState, event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.stopPropagation();
    const additive = event.shiftKey || event.metaKey || event.ctrlKey;
    const nextSelection = selectionForNodePointer(selectedIds, node.id, additive);
    setSelectedIds(nextSelection);
    if (pendingProposal) return;

    const starts = [...nextSelection]
      .map((nodeId) => nodeById.get(nodeId))
      .filter((item): item is GraphNodeState => Boolean(item))
      .map((item) => ({
        id: item.id,
        x: item.position.x,
        y: item.position.y,
      }));
    if (starts.length === 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    nodeDrag.current = {
      pointerId: event.pointerId,
      sx: event.clientX,
      sy: event.clientY,
      starts,
    };
  };

  const handleNodePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const currentDrag = nodeDrag.current;
    if (!currentDrag || currentDrag.pointerId !== event.pointerId) return;
    event.stopPropagation();
    const moved = moveNodeDrafts(currentDrag.starts, {
      x: (event.clientX - currentDrag.sx) / view.z,
      y: (event.clientY - currentDrag.sy) / view.z,
    });
    setDraftPositions((current) => ({ ...current, ...moved }));
  };

  const stopNodeDrag = (event: PointerEvent<HTMLDivElement>) => {
    const currentDrag = nodeDrag.current;
    if (!currentDrag || currentDrag.pointerId !== event.pointerId) return;
    event.stopPropagation();
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    nodeDrag.current = null;
  };

  const saveLayout = async () => {
    if (!onSaveLayout || !hasDirtyLayout || pendingProposal || layoutSaving) return;
    setLayoutSaving(true);
    try {
      await onSaveLayout(layoutUpdates);
      setDraftPositions({});
    } finally {
      setLayoutSaving(false);
    }
  };

  return (
    <section
      ref={canvasRef}
      className="p-canvas cv-bold"
      onClick={() => setSelectedIds(new Set())}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = {
          pointerId: event.pointerId,
          sx: event.clientX,
          sy: event.clientY,
          ox: view.x,
          oy: view.y,
        };
      }}
      onPointerMove={(event) => {
        const currentDrag = drag.current;
        if (!currentDrag || currentDrag.pointerId !== event.pointerId) return;
        const nextX = currentDrag.ox + event.clientX - currentDrag.sx;
        const nextY = currentDrag.oy + event.clientY - currentDrag.sy;
        setView((current) => ({
          ...current,
          x: nextX,
          y: nextY,
        }));
      }}
      onPointerCancel={stopDrag}
      onPointerUp={stopDrag}
      onWheel={handleWheel}
    >
      <div className="canvas-grid" />
      <div className="canvas-toolbar" onPointerDown={(event) => event.stopPropagation()}>
        <div className="mode-seg">
          <button className={activeMode === 'view' ? 'on' : ''} onClick={() => setMode('view')}>
            查看
          </button>
          <button className={activeMode === 'edit' ? 'on' : ''} onClick={() => setMode('edit')}>
            编辑
          </button>
          <button className={activeMode === 'review' ? 'on' : ''} onClick={() => setMode('review')}>
            审阅
          </button>
        </div>
        <span className="canvas-pill">
          <Icon n="layers" s={13} c="var(--text-3)" />
          {nodeCount} 节点 · {drawGraph.edges.length} 连线
        </span>
        <span className={pendingProposal ? 'pill pill--warn' : 'pill pill--off'}>
          <span className="led" />
          {pendingProposal ? '待确认的图变更 — 预览中' : runStatusLabel(run.status)}
        </span>
        {!pendingProposal && hasDirtyLayout && (
          <button
            className="layout-save"
            disabled={layoutSaving || !onSaveLayout}
            onClick={() => void saveLayout()}
          >
            {layoutSaving ? '保存中' : `保存布局 · ${layoutUpdates.length}`}
          </button>
        )}
      </div>
      <div
        className="world"
        style={{
          left: view.x,
          top: view.y,
          transform: `scale(${view.z})`,
        }}
      >
        <GraphEdges
          baseEdgeIds={baseEdgeIds}
          edges={drawGraph.edges}
          hasProposal={Boolean(pendingProposal)}
          nodeById={nodeById}
        />
        {displayNodes.map((node) => (
          <WorkflowNode
            key={node.id}
            diffState={nodeDiffState(
              node,
              baseComparableById.get(node.id),
              Boolean(pendingProposal),
            )}
            dirty={Boolean(draftPositions[node.id]) && !pendingProposal}
            locked={Boolean(pendingProposal)}
            node={node}
            selected={selectedIds.has(node.id)}
            stepState={stepStateByNodeId.get(node.id) ?? node.status}
            onPointerCancel={stopNodeDrag}
            onPointerDown={(event) => handleNodePointerDown(node, event)}
            onPointerMove={handleNodePointerMove}
            onPointerUp={stopNodeDrag}
          />
        ))}
      </div>
      {nodeCount === 0 && (
        <div className="empty-canvas">
          <div className="empty-card">
            <div className="empty-icon">
              <Icon n="layers" s={22} />
            </div>
            <div className="empty-title">空白工作流</div>
            <div className="empty-sub">先描述要设计的结果；需要自动化时再生成 workflow。</div>
          </div>
        </div>
      )}
      <div className="zoom-ctl" onPointerDown={(event) => event.stopPropagation()}>
        <button onClick={() => updateView((current) => ({ ...current, z: current.z - 0.1 }))}>
          -
        </button>
        <span>{Math.round(view.z * 100)}%</span>
        <button onClick={() => updateView((current) => ({ ...current, z: current.z + 0.1 }))}>
          +
        </button>
      </div>
      {minimapLayout && (
        <CanvasMinimap
          layout={minimapLayout}
          onNavigate={updateViewFromMinimap}
          view={view}
          viewportSize={viewportSize}
        />
      )}
      {selectedNode && (
        <GraphInspector node={selectedNode} onClose={() => setSelectedIds(new Set())} />
      )}
    </section>
  );
}
