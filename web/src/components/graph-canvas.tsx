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
import { fetchModelCatalog, fetchNodeCatalog, resolveImplementation } from '../api';
import { portColor } from '../icons';
import {
  CANVAS_PRESENCE_HEARTBEAT_MS,
  CANVAS_PRESENCE_INTERVAL_MS,
  LOCAL_CANVAS_ACTOR,
} from '../canvas-presence';
import type { CanvasPresence, ImplementationResolution, ModelCatalog, NodeCatalog } from '../types';
import { connectionPath } from './graph-canvas-connections';
import { useCanvasConnectionController } from './graph-canvas-connection-controller';
import {
  CanvasCollaborationWorld,
  CanvasCommentsPanel,
  ConnectedCanvasCollaborationWorld,
} from './graph-canvas-collaboration';
import { outputsByCanvasNode } from './graph-canvas-artifacts';
import { canvasCapabilities } from './graph-canvas-capabilities';
import { createCanvasEditActions } from './graph-canvas-edit-actions';
import { GraphEdges } from './graph-canvas-edges';
import { GraphInspector, GraphSelectionInspector } from './graph-canvas-inspector';
import {
  applyPositionDrafts,
  selectionForNodePointer,
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
import {
  buildComparableNodeMap,
  buildEdgeSignatureSet,
  buildNodeMap,
  buildRunStepStateMap,
  nodeDiffState,
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
import { createLatestFrameScheduler } from './latest-frame-scheduler';
import { createLatestAsyncPublisher } from './latest-async-publisher';

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
  presenceByActor,
  run,
  workflowGraph,
  onCommentOp,
  onCreateProposal,
  onPresenceChange,
  onRequestNodeProposal,
  onSelectOutput,
  onSelectionChange,
  onSetParam,
  onStartFromPrompt,
  outputs,
}: GraphCanvasProps) {
  const canvasRef = useRef<HTMLElement | null>(null);
  const canvasRectRef = useRef<{
    left: number;
    top: number;
    width: number;
    height: number;
  } | null>(null);
  const [view, setView] = useState<ViewState>(DEFAULT_GRAPH_VIEW);
  const [viewportSize, setViewportSize] = useState<ViewportSize>({ width: 900, height: 640 });
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [draftPositions, setDraftPositions] = useState<PositionDrafts>({});
  const [draftSizes, setDraftSizes] = useState<SizeDrafts>({});
  const [catalog, setCatalog] = useState<NodeCatalog | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [modelCatalog, setModelCatalog] = useState<ModelCatalog | null>(null);
  const [modelCatalogError, setModelCatalogError] = useState<string | null>(null);
  const [resolution, setResolution] = useState<ImplementationResolution | null>(null);
  const [selectionDrag, setSelectionDrag] = useState<SelectionDragState | null>(null);
  const [clipboardStatus, setClipboardStatus] = useState<string | null>(null);
  const localCursorRef = useRef<Point | null>(null);
  const onPresenceChangeRef = useRef(onPresenceChange);
  const selectedIdListRef = useRef<string[]>([]);
  const viewRef = useRef<ViewState>(DEFAULT_GRAPH_VIEW);
  const drag = useRef<DragState | null>(null);
  const [panScheduler] = useState(() =>
    createLatestFrameScheduler<{ x: number; y: number }>((next) => {
      setView((current) => ({ ...current, ...next }));
    }),
  );
  const [presencePublisher] = useState(() =>
    createLatestAsyncPublisher<CanvasPresence>(
      (presence) => onPresenceChangeRef.current?.(presence),
      CANVAS_PRESENCE_INTERVAL_MS,
    ),
  );
  const suppressNextClick = useRef(false);
  const sourceGraph = canvasGraph ?? graph;
  const drawGraph = pendingProposal?.previewGraph ?? sourceGraph;
  const activeMode = pendingProposal ? 'review' : onCreateProposal ? 'edit' : 'view';
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
  const selectedCapability = selectedNode
    ? (definitionByType.get(selectedNode.nodeType)?.capability ?? null)
    : null;

  useEffect(() => {
    return () => panScheduler.cancel();
  }, [panScheduler]);

  useEffect(() => {
    if (!selectedCapability) {
      setResolution(null);
      return;
    }
    const controller = new AbortController();
    setResolution(null);
    resolveImplementation(selectedCapability, undefined, controller.signal)
      .then((outcome) => {
        if (!controller.signal.aborted) setResolution(outcome);
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setResolution({
            status: 'unresolvable',
            code: 'UNKNOWN',
            message: '解析请求失败',
            recoverable: false,
          });
        }
      });
    return () => {
      controller.abort();
    };
  }, [selectedCapability]);
  const selectedNodes = useMemo(
    () => displayNodes.filter((node) => selectedIds.has(node.id)),
    [displayNodes, selectedIds],
  );
  const selectedIdList = useMemo(() => [...selectedIds], [selectedIds]);
  onPresenceChangeRef.current = onPresenceChange;
  selectedIdListRef.current = selectedIdList;
  viewRef.current = view;
  const nodeCount = displayNodes.length;
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
  const cacheCanvasRect = useCallback((element: HTMLElement | null = canvasRef.current) => {
    const rect = element?.getBoundingClientRect();
    if (!rect) return null;
    const cached = { left: rect.left, top: rect.top, width: rect.width, height: rect.height };
    canvasRectRef.current = cached;
    return cached;
  }, []);
  const worldPointFromClient = useCallback(
    (clientX: number, clientY: number): Point => {
      const rect = canvasRectRef.current ?? cacheCanvasRect();
      if (!rect) return { x: 0, y: 0 };
      const currentView = viewRef.current;
      return {
        x: (clientX - rect.left - currentView.x) / currentView.z,
        y: (clientY - rect.top - currentView.y) / currentView.z,
      };
    },
    [cacheCanvasRect],
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
  const schedulePresence = useCallback((immediate = false) => {
    if (!onPresenceChangeRef.current) return;
    const currentView = viewRef.current;
    presencePublisher.schedule({
      actor: LOCAL_CANVAS_ACTOR,
      cursor: localCursorRef.current,
      selection: { nodeIds: selectedIdListRef.current, edgeIds: [] },
      viewport: { x: currentView.x, y: currentView.y, zoom: currentView.z },
    });
    if (immediate) presencePublisher.flush();
  }, [presencePublisher]);

  useEffect(() => {
    panScheduler.cancel();
    setView(loadGraphCanvasView(workspaceId));
    setSelectedIds(new Set());
    setDraftPositions({});
    setDraftSizes({});
    setSelectionDrag(null);
    connection.reset();
    setClipboardStatus(null);
    nodeDrag.reset();
    resetNodeResize();
  }, [connection.reset, nodeDrag.reset, panScheduler, resetNodeResize, workspaceId]);

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
    schedulePresence();
  }, [schedulePresence, selectedIdList, view.x, view.y, view.z]);

  useEffect(() => {
    const heartbeat = setInterval(schedulePresence, CANVAS_PRESENCE_HEARTBEAT_MS);
    return () => {
      clearInterval(heartbeat);
      presencePublisher.cancel();
    };
  }, [presencePublisher, schedulePresence, workspaceId]);

  useEffect(() => {
    const current = canvasRef.current;
    if (!current) return;

    const updateSize = () => {
      const rect = cacheCanvasRect(current);
      if (!rect) return;
      if (rect.width > 0 && rect.height > 0) {
        setViewportSize({ width: rect.width, height: rect.height });
      }
    };
    updateSize();
    if (typeof window !== 'undefined') window.addEventListener('scroll', updateSize, true);
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(updateSize);
    observer?.observe(current);
    return () => {
      observer?.disconnect();
      if (typeof window !== 'undefined') window.removeEventListener('scroll', updateSize, true);
    };
  }, [cacheCanvasRect]);

  useEffect(() => {
    const timer = setTimeout(() => saveGraphCanvasView(workspaceId, view), 180);
    return () => clearTimeout(timer);
  }, [view, workspaceId]);

  useEffect(() => {
    const controller = new AbortController();
    fetchNodeCatalog(controller.signal)
      .then((nextCatalog) => {
        if (!controller.signal.aborted) {
          setCatalog(nextCatalog);
          setCatalogError(null);
        }
      })
      .catch((error) => {
        if (!controller.signal.aborted) {
          setCatalogError(error instanceof Error ? error.message : 'node catalog request failed');
        }
      });
    return () => {
      controller.abort();
    };
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    fetchModelCatalog(controller.signal)
      .then((nextCatalog) => {
        if (!controller.signal.aborted) {
          setModelCatalog(nextCatalog);
          setModelCatalogError(null);
        }
      })
      .catch((error) => {
        if (!controller.signal.aborted) {
          setModelCatalogError(
            error instanceof Error ? error.message : 'model catalog request failed',
          );
        }
      });
    return () => {
      controller.abort();
    };
  }, []);

  const updateView = useCallback((next: ViewState | ((current: ViewState) => ViewState)) => {
    setView((current) => normalizeView(typeof next === 'function' ? next(current) : next));
  }, []);

  const handleWheel = (event: WheelEvent<HTMLElement>) => {
    if (!capabilities.zoom) return;
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest('.zoom-ctl,.inspector,.canvas-minimap')) return;
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
    panScheduler.cancel();
    setView((current) => ({
      ...current,
      x: currentDrag.ox + event.clientX - currentDrag.sx,
      y: currentDrag.oy + event.clientY - currentDrag.sy,
    }));
    drag.current = null;
  };

  const canvasLocalPoint = (event: PointerEvent<HTMLElement>): Point => {
    const rect = canvasRectRef.current ?? cacheCanvasRect(event.currentTarget);
    if (!rect) return { x: 0, y: 0 };
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  };

  const shouldStartSelectionDrag = (event: PointerEvent<HTMLElement>) =>
    capabilities.select &&
    (activeMode === 'edit' || event.shiftKey || event.metaKey || event.ctrlKey);

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
      aria-label="Workflow graph canvas"
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
      onPointerEnter={(event) => {
        cacheCanvasRect(event.currentTarget);
      }}
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
        cacheCanvasRect(event.currentTarget);
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
        const worldPoint = worldPointFromClient(event.clientX, event.clientY);
        localCursorRef.current = worldPoint;
        schedulePresence();
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
                  currentPoint: worldPoint,
                }
              : current,
          );
          return;
        }
        if (selectionDrag?.pointerId === event.pointerId) {
          const currentPoint = canvasLocalPoint(event);
          setSelectionDrag((current) =>
            current ? { ...current, current: currentPoint } : current,
          );
          return;
        }
        const currentDrag = drag.current;
        if (!currentDrag || currentDrag.pointerId !== event.pointerId) return;
        const nextX = currentDrag.ox + event.clientX - currentDrag.sx;
        const nextY = currentDrag.oy + event.clientY - currentDrag.sy;
        panScheduler.schedule({ x: nextX, y: nextY });
      }}
      onPointerLeave={() => {
        localCursorRef.current = null;
        schedulePresence(true);
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
        modelCatalog={modelCatalog}
        modelCatalogError={modelCatalogError}
        onAddNode={editActions.addNode}
      />
      <CanvasStatusToast
        clipboardStatus={clipboardStatus}
        connectionStatus={connection.status}
        pendingProposal={Boolean(pendingProposal)}
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
            workflowNode={workflowGraph?.nodes[node.id]}
            artifactOutputs={outputsByNodeId.get(node.id) ?? []}
            portHighlights={connection.portHighlights}
            resizable={capabilities.resize}
            selected={selectedIds.has(node.id)}
            stepState={stepStateByNodeId.get(node.id) ?? node.status}
            onSelectOutput={onSelectOutput}
            onKeyboardSelect={(additive) => {
              setSelectedIds((current) =>
                selectionForNodePointer(current, node.id, additive),
              );
            }}
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
        {presenceByActor ? (
          <CanvasCollaborationWorld
            comments={comments}
            hiddenActorId={LOCAL_CANVAS_ACTOR.actorId}
            nodes={displayNodes}
            presenceByActor={presenceByActor}
          />
        ) : (
          <ConnectedCanvasCollaborationWorld comments={comments} nodes={displayNodes} />
        )}
      </div>
      {nodeCount === 0 && (
        <EmptyCanvas disabled={connectionDisabled} onPrompt={onStartFromPrompt} />
      )}
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
          resolution={resolution}
          onClose={() => setSelectedIds(new Set())}
          onRequestProposal={capabilities.move ? onRequestNodeProposal : undefined}
          onSetParam={capabilities.move ? onSetParam : undefined}
          view={view}
          viewportSize={viewportSize}
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

function CanvasStatusToast({
  clipboardStatus,
  connectionStatus,
  pendingProposal,
}: {
  clipboardStatus: string | null;
  connectionStatus: string | null;
  pendingProposal: boolean;
}) {
  const message = pendingProposal
    ? '待确认的图变更 · 预览中'
    : connectionStatus ?? clipboardStatus;
  if (!message) return null;
  return <div className="canvas-status-toast" role="status" aria-live="polite">{message}</div>;
}
