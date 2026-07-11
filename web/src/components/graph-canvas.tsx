import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type WheelEvent,
} from 'react';
import { fetchNodeCatalog } from '../api';
import { portColor } from '../icons';
import type { NodeCatalog } from '../types';
import { connectionPath } from './graph-canvas-connections';
import { useCanvasConnectionController } from './graph-canvas-connection-controller';
import {
  CanvasCollaborationWorld,
  CanvasCommentsPanel,
} from './graph-canvas-collaboration';
import { outputsByCanvasNode } from './graph-canvas-artifacts';
import { canvasCapabilities } from './graph-canvas-capabilities';
import { createCanvasEditActions } from './graph-canvas-edit-actions';
import { GraphEdges } from './graph-canvas-edges';
import { GraphInspector, GraphSelectionInspector } from './graph-canvas-inspector';
import {
  applyPositionDrafts,
  positionUpdatesFromDrafts,
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
import { useCanvasNodeDragController } from './graph-canvas-node-drag-controller';
import { EmptyCanvas, ZoomControls } from './graph-canvas-overlays';
import { GraphCanvasToolbar } from './graph-canvas-toolbar';
import {
  buildComparableNodeMap,
  buildEdgeSignatureSet,
  buildNodeMap,
  buildRunStepStateMap,
  nodeDiffState,
  runStatusLabel,
} from './graph-canvas-rendering';
import {
  applySizeDrafts,
  useNodeResizeController,
  type SizeDrafts,
} from './graph-canvas-resize';
import {
  fitViewToNodes,
  graphShortcutFromEvent,
  isEditableShortcutTarget,
  mergeSelection,
  selectedIdsInWorldRect,
  selectionRectFromPoints,
  worldRectFromLocalRect,
  type Point,
} from './graph-canvas-selection';
import type {
  DragState,
  GraphCanvasProps,
  SelectionDragState,
} from './graph-canvas-types';
import { NodeLibrary } from './node-library';

export { DEFAULT_GRAPH_VIEW, GRAPH_CANVAS_VIEW_STORAGE_PREFIX, clampZoom,
  computeMinimapLayout, loadGraphCanvasView, minimapViewportRect, normalizeView,
  saveGraphCanvasView, viewForMinimapPoint, viewStorageKey, zoomViewAtPoint,
} from './graph-canvas-navigation';
export type { MinimapLayout, ViewState, ViewportSize } from './graph-canvas-navigation';

export function GraphCanvas({
  workspaceId,
  versionId,
  graph,
  canvasGraph,
  comments = [],
  pendingProposal,
  presenceByActor = {},
  run,
  workflowGraph,
  onCommentOp,
  onSaveLayout,
  onCreateProposal,
  onPresenceChange,
  onQueueRun,
  onRequestNodeProposal,
  onSelectOutput,
  onSelectionChange,
  onSetParam,
  outputs,
  queueRunDisabled,
}: GraphCanvasProps) {
  const canvasRef = useRef<HTMLElement | null>(null);
  const [view, setView] = useState<ViewState>(DEFAULT_GRAPH_VIEW);
  const [viewportSize, setViewportSize] = useState<ViewportSize>({ width: 900, height: 640 });
  const [mode, setMode] = useState<'view' | 'edit' | 'review'>('view');
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [draftPositions, setDraftPositions] = useState<PositionDrafts>({});
  const [draftSizes, setDraftSizes] = useState<SizeDrafts>({});
  const [layoutSaving, setLayoutSaving] = useState(false);
  const [catalog, setCatalog] = useState<NodeCatalog | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [selectionDrag, setSelectionDrag] = useState<SelectionDragState | null>(null);
  const [clipboardStatus, setClipboardStatus] = useState<string | null>(null);
  const [localCursor, setLocalCursor] = useState<Point | null>(null);
  const drag = useRef<DragState | null>(null);
  const suppressNextClick = useRef(false);
  const sourceGraph = canvasGraph ?? graph;
  const drawGraph = pendingProposal?.previewGraph ?? sourceGraph;
  const activeMode = pendingProposal ? 'review' : mode;
  const capabilities = canvasCapabilities(activeMode, Boolean(onCreateProposal));
  const connectionDisabled = !capabilities.connect;
  const displayNodes = useMemo(
    () => applySizeDrafts(applyPositionDrafts(drawGraph.nodes, draftPositions), draftSizes),
    [drawGraph.nodes, draftPositions, draftSizes],
  );
  const baseComparableById = useMemo(
    () => buildComparableNodeMap(sourceGraph.nodes),
    [sourceGraph.nodes],
  );
  const nodeById = useMemo(() => buildNodeMap(displayNodes), [displayNodes]);
  const outputsByNodeId = useMemo(() => outputsByCanvasNode(outputs), [outputs]);
  const baseEdgeIds = useMemo(() => buildEdgeSignatureSet(sourceGraph.edges), [sourceGraph.edges]);
  const stepStateByNodeId = useMemo(() => buildRunStepStateMap(run.steps), [run.steps]);
  const definitionByType = useMemo(
    () => new Map((catalog?.nodes ?? []).map((definition) => [definition.type, definition] as const)),
    [catalog],
  );
  const selectedNodeId = selectedIds.values().next().value as string | undefined;
  const selectedNode = selectedNodeId ? nodeById.get(selectedNodeId) : null;
  const selectedWorkflowNode = selectedNodeId ? workflowGraph?.nodes[selectedNodeId] : undefined;
  const selectedNodes = useMemo(
    () => displayNodes.filter((node) => selectedIds.has(node.id)),
    [displayNodes, selectedIds],
  );
  const selectedIdList = useMemo(() => [...selectedIds], [selectedIds]);
  const nodeCount = displayNodes.length;
  const layoutUpdates = useMemo(
    () => (pendingProposal ? [] : positionUpdatesFromDrafts(sourceGraph.nodes, draftPositions)),
    [draftPositions, pendingProposal, sourceGraph.nodes],
  );
  const hasDirtyLayout = layoutUpdates.length > 0;
  const minimapLayout = useMemo(
    () => computeMinimapLayout(displayNodes, { width: MINIMAP_WIDTH, height: MINIMAP_HEIGHT }),
    [displayNodes],
  );
  const editActions = createCanvasEditActions({
    capabilities,
    drawGraph,
    onCreateProposal,
    setClipboardStatus,
    versionId,
    view,
    viewportSize,
    workflowGraph,
  });
  const worldPointFromClient = useCallback(
    (clientX: number, clientY: number): Point => {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return { x: 0, y: 0 };
      return {
        x: (clientX - rect.left - view.x) / view.z,
        y: (clientY - rect.top - view.y) / view.z,
      };
    },
    [view.x, view.y, view.z],
  );
  const connection = useCanvasConnectionController({
    canConnect: capabilities.connect,
    definitions: definitionByType,
    edges: drawGraph.edges,
    nodes: displayNodes,
    onCreateProposal,
    versionId,
    worldPointFromClient,
  });
  const {
    handleNodeResizeMove,
    resetNodeResize,
    startNodeResize,
    stopNodeResize,
  } = useNodeResizeController({
    connectionDisabled: !capabilities.resize,
    onCreateProposal,
    pendingProposal: Boolean(pendingProposal),
    setConnectionStatus: connection.setStatus,
    setDraftSizes,
    setSelectedIds,
    sourceNodes: sourceGraph.nodes,
    versionId,
    viewZoom: view.z,
  });
  const nodeDrag = useCanvasNodeDragController({
    baseNodes: graph.nodes,
    canMove: capabilities.move,
    nodeById,
    onCreateProposal,
    pendingProposal: Boolean(pendingProposal),
    selectedIds,
    setDraftPositions,
    setSelectedIds,
    setStatus: connection.setStatus,
    versionId,
    viewZoom: view.z,
  });

  useEffect(() => {
    setView(loadGraphCanvasView(workspaceId));
    setSelectedIds(new Set());
    setDraftPositions({});
    setDraftSizes({});
    setSelectionDrag(null);
    connection.reset();
    setClipboardStatus(null);
    nodeDrag.reset();
    resetNodeResize();
  }, [connection.reset, nodeDrag.reset, resetNodeResize, workspaceId]);

  useEffect(() => {
    setDraftPositions({});
    setDraftSizes({});
    setSelectionDrag(null);
    connection.reset();
    setClipboardStatus(null);
    nodeDrag.reset();
    resetNodeResize();
  }, [connection.reset, nodeDrag.reset, pendingProposal?.id, resetNodeResize, versionId]);

  useEffect(() => {
    if (!capabilities.move) setDraftPositions({});
    if (!capabilities.resize) { resetNodeResize(); setDraftSizes({}); }
  }, [capabilities.move, capabilities.resize, resetNodeResize]);

  useEffect(() => {
    onSelectionChange?.(selectedIdList);
  }, [onSelectionChange, selectedIdList]);

  useEffect(() => {
    if (!onPresenceChange) return;
    const timer = setTimeout(() => {
      onPresenceChange({
        actor: { actorId: 'local', displayName: 'Local user' },
        cursor: localCursor,
        selection: { nodeIds: selectedIdList, edgeIds: [] },
        viewport: { x: view.x, y: view.y, zoom: view.z },
      });
    }, 180);
    return () => clearTimeout(timer);
  }, [localCursor, onPresenceChange, selectedIdList, view.x, view.y, view.z]);

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

  useEffect(() => {
    let cancelled = false;
    fetchNodeCatalog()
      .then((nextCatalog) => {
        if (!cancelled) {
          setCatalog(nextCatalog);
          setCatalogError(null);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setCatalogError(error instanceof Error ? error.message : 'node catalog request failed');
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const updateView = useCallback((next: ViewState | ((current: ViewState) => ViewState)) => {
    setView((current) => normalizeView(typeof next === 'function' ? next(current) : next));
  }, []);

  const handleWheel = (event: WheelEvent<HTMLElement>) => {
    if (!capabilities.zoom) return;
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

  const canvasLocalPoint = (event: PointerEvent<HTMLElement>): Point => {
    const rect = event.currentTarget.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  };

  const shouldStartSelectionDrag = (event: PointerEvent<HTMLElement>) =>
    capabilities.select &&
    (activeMode === 'edit' || event.shiftKey || event.metaKey || event.ctrlKey);

  const saveLayout = async () => {
    if (!capabilities.move || !onSaveLayout || !hasDirtyLayout || pendingProposal || layoutSaving) return;
    setLayoutSaving(true);
    try {
      await onSaveLayout(layoutUpdates);
      setDraftPositions({});
    } finally {
      setLayoutSaving(false);
    }
  };

  const completeSelectionDrag = (event: PointerEvent<HTMLElement>) => {
    const current = selectionDrag;
    if (!current || current.pointerId !== event.pointerId) return false;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    const rect = selectionRectFromPoints(current.start, current.current);
    const worldRect = worldRectFromLocalRect(rect, view);
    const hitIds = selectedIdsInWorldRect(displayNodes, worldRect);
    setSelectedIds(mergeSelection(current.baseIds, hitIds, current.additive));
    setSelectionDrag(null);
    suppressNextClick.current = rect.width > 2 || rect.height > 2;
    return true;
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (isEditableShortcutTarget(event.target)) return;
    const shortcut = graphShortcutFromEvent(event);
    if (!shortcut) return;
    event.preventDefault();
    if (shortcut === 'clear_selection') {
      setSelectedIds(new Set());
      setClipboardStatus(null);
    }
    if (shortcut === 'select_all') {
      setSelectedIds(new Set(displayNodes.map((node) => node.id)));
    }
    if (shortcut === 'fit_view') {
      updateView(fitViewToNodes(displayNodes, viewportSize));
    }
    if (shortcut === 'copy_selection') {
      void editActions.copySelection(selectedNodes);
    }
    if (shortcut === 'paste_selection') {
      void editActions.pasteSelection();
    }
    if (shortcut === 'delete_selection') {
      editActions.deleteSelection(selectedIds);
    }
  };

  return (
    <section
      ref={canvasRef}
      className="p-canvas cv-bold"
      tabIndex={0}
      onClick={() => {
        if (suppressNextClick.current) {
          suppressNextClick.current = false;
          return;
        }
        setSelectedIds(new Set());
        connection.setStatus(null);
        setClipboardStatus(null);
      }}
      onKeyDown={handleKeyDown}
      onDragOver={(event) => {
        if (!connectionDisabled && event.dataTransfer.types.includes('application/x-helixflow-node-type')) {
          event.preventDefault();
          event.dataTransfer.dropEffect = 'copy';
        }
      }}
      onDrop={(event) => {
        const nodeType = event.dataTransfer.getData('application/x-helixflow-node-type');
        const definition = definitionByType.get(nodeType);
        if (!definition || connectionDisabled) return;
        event.preventDefault();
        editActions.addNode(definition, worldPointFromClient(event.clientX, event.clientY));
      }}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        event.currentTarget.focus();
        if (shouldStartSelectionDrag(event)) {
          const start = canvasLocalPoint(event);
          setSelectionDrag({
            pointerId: event.pointerId,
            start,
            current: start,
            additive: event.shiftKey || event.metaKey || event.ctrlKey,
            baseIds: new Set(selectedIds),
          });
          drag.current = null;
          return;
        }
        if (!capabilities.pan) return;
        drag.current = {
          pointerId: event.pointerId,
          sx: event.clientX,
          sy: event.clientY,
          ox: view.x,
          oy: view.y,
        };
      }}
      onPointerMove={(event) => {
        setLocalCursor(worldPointFromClient(event.clientX, event.clientY));
        if (connection.drag?.pointerId === event.pointerId) {
          if (!capabilities.connect) {
            connection.setDrag(null);
            return;
          }
          event.preventDefault();
          event.stopPropagation();
          connection.setDrag((current) =>
            current
              ? {
                  ...current,
                  currentPoint: worldPointFromClient(event.clientX, event.clientY),
                }
              : current,
          );
          return;
        }
        if (selectionDrag?.pointerId === event.pointerId) {
          setSelectionDrag((current) =>
            current ? { ...current, current: canvasLocalPoint(event) } : current,
          );
          return;
        }
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
      onPointerCancel={(event) => {
        if (connection.cancel(event)) return;
        if (completeSelectionDrag(event)) return;
        stopDrag(event);
      }}
      onPointerUp={(event) => {
        if (connection.complete(event)) return;
        if (completeSelectionDrag(event)) return;
        stopDrag(event);
      }}
      onWheel={handleWheel}
    >
      <div className="canvas-grid" />
      <NodeLibrary
        catalog={catalog}
        disabled={connectionDisabled}
        error={catalogError}
        onAddNode={editActions.addNode}
      />
      <GraphCanvasToolbar
        activeMode={activeMode}
        clipboardStatus={clipboardStatus}
        connectionStatus={connection.status}
        edgeCount={drawGraph.edges.length}
        hasDirtyLayout={hasDirtyLayout}
        layoutSaving={layoutSaving}
        layoutUpdateCount={layoutUpdates.length}
        nodeCount={nodeCount}
        onSaveLayout={() => void saveLayout()}
        onQueueRun={onQueueRun}
        pendingProposal={Boolean(pendingProposal)}
        queueRunDisabled={queueRunDisabled}
        runStatusLabel={runStatusLabel(run.status)}
        saveLayoutDisabled={!capabilities.move || layoutSaving || !onSaveLayout}
        setMode={setMode}
      />
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
          onDisconnectEdge={connectionDisabled ? undefined : connection.disconnect}
        />
        {connection.drag && (
          <svg className="edge-svg connection-drag-svg">
            <path
              className="connection-drag-path"
              d={connectionPath(connection.drag.sourcePoint, connection.drag.currentPoint)}
              fill="none"
              stroke={portColor(connection.drag.source.type)}
              strokeLinecap="round"
              strokeWidth="3"
            />
          </svg>
        )}
        {displayNodes.map((node) => (
          <WorkflowNode
            key={node.id}
            connectionDisabled={connectionDisabled}
            definition={definitionByType.get(node.nodeType)}
            diffState={nodeDiffState(
              node,
              baseComparableById.get(node.id),
              Boolean(pendingProposal),
            )}
            dirty={Boolean(draftPositions[node.id]) && !pendingProposal}
            locked={Boolean(pendingProposal)}
            node={node}
            artifactOutputs={outputsByNodeId.get(node.id) ?? []}
            portHighlights={connection.portHighlights}
            resizable={capabilities.resize}
            selected={selectedIds.has(node.id)}
            stepState={stepStateByNodeId.get(node.id) ?? node.status}
            onSelectOutput={onSelectOutput}
            onOutputPortPointerDown={connection.start}
            onPointerCancel={nodeDrag.stop}
            onPointerDown={(event) => {
              if (capabilities.select) nodeDrag.start(node, event);
            }}
            onPointerMove={nodeDrag.move}
            onPointerUp={nodeDrag.stop}
            onResizePointerCancel={stopNodeResize}
            onResizePointerDown={(event) => startNodeResize(node, event)}
            onResizePointerMove={handleNodeResizeMove}
            onResizePointerUp={stopNodeResize}
          />
        ))}
        <CanvasCollaborationWorld
          comments={comments}
          nodes={displayNodes}
          presenceByActor={presenceByActor}
        />
      </div>
      {nodeCount === 0 && <EmptyCanvas />}
      {selectionDrag && (
        <span
          className="selection-rect"
          style={selectionRectFromPoints(selectionDrag.start, selectionDrag.current)}
        />
      )}
      <ZoomControls setView={updateView} view={view} />
      {minimapLayout && (
        <CanvasMinimap
          layout={minimapLayout}
          onNavigate={updateViewFromMinimap}
          view={view}
          viewportSize={viewportSize}
        />
      )}
      {selectedNodes.length > 1 && (
        <GraphSelectionInspector
          nodes={selectedNodes}
          onClose={() => setSelectedIds(new Set())}
          onCopy={() => void editActions.copySelection(selectedNodes)}
          onDelete={() => editActions.deleteSelection(selectedIds)}
          view={view}
          viewportSize={viewportSize}
        />
      )}
      {selectedNodes.length === 1 && selectedNode && (
        <GraphInspector
          catalogError={catalogError}
          definition={definitionByType.get(selectedNode.nodeType)}
          node={selectedNode}
          onClose={() => setSelectedIds(new Set())}
          onRequestProposal={capabilities.move ? onRequestNodeProposal : undefined}
          onSetParam={capabilities.move ? onSetParam : undefined}
          workflowNode={selectedWorkflowNode}
        />
      )}
      <CanvasCommentsPanel
        comments={comments}
        edges={drawGraph.edges}
        nodes={displayNodes}
        onCommentOp={capabilities.move ? onCommentOp : undefined}
        selectedNodeId={selectedNodeId}
        view={view}
        viewportSize={viewportSize}
      />
    </section>
  );
}
