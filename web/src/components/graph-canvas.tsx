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
import { Icon, portColor } from '../icons';
import { buildMoveNodeEditInput } from '../workbench-edit-session';
import type {
  GraphNodeState,
  LayoutPositionUpdate,
  ManualProposalInput,
  NodeCatalog,
  WorkbenchState,
} from '../types';
import {
  buildConnectionProposalInput,
  buildPortHighlights,
  connectionPath,
  edgeToRemoveOp,
  findInputConnection,
  portAnchorPoint,
  portTypeMatches,
  type ConnectionPort,
  type PortHighlight,
} from './graph-canvas-connections';
import {
  confirmReplace,
  portDropTargetFromPoint,
  releaseConnectionCapture,
} from './graph-canvas-connection-events';
import { createCanvasEditActions } from './graph-canvas-edit-actions';
import { GraphEdges } from './graph-canvas-edges';
import { GraphInspector, GraphSelectionInspector } from './graph-canvas-inspector';
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
import { NodeLibrary } from './node-library';

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
  workflowGraph?: WorkbenchState['workflowGraph'];
  onSaveLayout?: (positions: LayoutPositionUpdate[]) => Promise<void>;
  onCreateProposal?: (input: ManualProposalInput) => Promise<void>;
  onRequestNodeProposal?: (nodeId: string) => Promise<void>;
  onSelectionChange?: (nodeIds: string[]) => void;
  onSetParam?: (nodeId: string, key: string, value: unknown) => Promise<void>;
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

type SelectionDragState = {
  pointerId: number;
  start: Point;
  current: Point;
  additive: boolean;
  baseIds: Set<string>;
};

type ConnectionDragState = {
  pointerId: number;
  source: ConnectionPort;
  sourcePoint: Point;
  currentPoint: Point;
};

export function GraphCanvas({
  workspaceId,
  versionId,
  graph,
  pendingProposal,
  run,
  workflowGraph,
  onSaveLayout,
  onCreateProposal,
  onRequestNodeProposal,
  onSelectionChange,
  onSetParam,
}: GraphCanvasProps) {
  const canvasRef = useRef<HTMLElement | null>(null);
  const [view, setView] = useState<ViewState>(DEFAULT_GRAPH_VIEW);
  const [viewportSize, setViewportSize] = useState<ViewportSize>({ width: 900, height: 640 });
  const [mode, setMode] = useState<'view' | 'edit' | 'review'>('view');
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [draftPositions, setDraftPositions] = useState<PositionDrafts>({});
  const [layoutSaving, setLayoutSaving] = useState(false);
  const [catalog, setCatalog] = useState<NodeCatalog | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [selectionDrag, setSelectionDrag] = useState<SelectionDragState | null>(null);
  const [connectionDrag, setConnectionDrag] = useState<ConnectionDragState | null>(null);
  const [connectionStatus, setConnectionStatus] = useState<string | null>(null);
  const [clipboardStatus, setClipboardStatus] = useState<string | null>(null);
  const drag = useRef<DragState | null>(null);
  const nodeDrag = useRef<NodeDragState | null>(null);
  const suppressNextClick = useRef(false);
  const drawGraph = pendingProposal?.previewGraph ?? graph;
  const activeMode = pendingProposal ? 'review' : mode;
  const connectionDisabled = !onCreateProposal || activeMode === 'review';
  const displayNodes = useMemo(
    () => applyPositionDrafts(drawGraph.nodes, draftPositions),
    [drawGraph.nodes, draftPositions],
  );
  const baseComparableById = useMemo(() => buildComparableNodeMap(graph.nodes), [graph.nodes]);
  const nodeById = useMemo(() => buildNodeMap(displayNodes), [displayNodes]);
  const baseEdgeIds = useMemo(() => buildEdgeSignatureSet(graph.edges), [graph.edges]);
  const stepStateByNodeId = useMemo(() => buildRunStepStateMap(run.steps), [run.steps]);
  const definitionByType = useMemo(
    () => new Map((catalog?.nodes ?? []).map((definition) => [definition.type, definition] as const)),
    [catalog],
  );
  const portHighlights = useMemo(
    () =>
      connectionDrag
        ? buildPortHighlights(displayNodes, definitionByType, connectionDrag.source, drawGraph.edges)
        : new Map<string, PortHighlight>(),
    [connectionDrag, definitionByType, displayNodes, drawGraph.edges],
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
    () => (pendingProposal ? [] : positionUpdatesFromDrafts(graph.nodes, draftPositions)),
    [draftPositions, graph.nodes, pendingProposal],
  );
  const hasDirtyLayout = layoutUpdates.length > 0;
  const minimapLayout = useMemo(
    () => computeMinimapLayout(displayNodes, { width: MINIMAP_WIDTH, height: MINIMAP_HEIGHT }),
    [displayNodes],
  );
  const editActions = createCanvasEditActions({
    disabled: connectionDisabled,
    drawGraph,
    onCreateProposal,
    setClipboardStatus,
    versionId,
    view,
    viewportSize,
    workflowGraph,
  });

  useEffect(() => {
    setView(loadGraphCanvasView(workspaceId));
    setSelectedIds(new Set());
    setDraftPositions({});
    setSelectionDrag(null);
    setConnectionDrag(null);
    setConnectionStatus(null);
    setClipboardStatus(null);
    nodeDrag.current = null;
  }, [workspaceId]);

  useEffect(() => {
    setDraftPositions({});
    setSelectionDrag(null);
    setConnectionDrag(null);
    setConnectionStatus(null);
    setClipboardStatus(null);
    nodeDrag.current = null;
  }, [versionId, pendingProposal?.id]);

  useEffect(() => {
    onSelectionChange?.(selectedIdList);
  }, [onSelectionChange, selectedIdList]);

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

  const worldPointFromClient = (clientX: number, clientY: number): Point => {
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return { x: 0, y: 0 };
    return {
      x: (clientX - rect.left - view.x) / view.z,
      y: (clientY - rect.top - view.y) / view.z,
    };
  };

  const shouldStartSelectionDrag = (event: PointerEvent<HTMLElement>) =>
    activeMode === 'edit' || event.shiftKey || event.metaKey || event.ctrlKey;

  const startConnectionDrag = (
    node: GraphNodeState,
    port: { name: string; type: string },
    index: number,
    event: PointerEvent<HTMLSpanElement>,
  ) => {
    if (connectionDisabled || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    setConnectionStatus(null);
    setConnectionDrag({
      pointerId: event.pointerId,
      source: { nodeId: node.id, port: port.name, type: port.type },
      sourcePoint: portAnchorPoint(node, 'output', index),
      currentPoint: worldPointFromClient(event.clientX, event.clientY),
    });
  };

  const cancelConnectionDrag = (event: PointerEvent<HTMLElement>) => {
    const current = connectionDrag;
    if (!current || current.pointerId !== event.pointerId) return false;
    releaseConnectionCapture(event);
    setConnectionDrag(null);
    setConnectionStatus('连线已取消');
    return true;
  };

  const completeConnectionDrag = (event: PointerEvent<HTMLElement>) => {
    const current = connectionDrag;
    if (!current || current.pointerId !== event.pointerId) return false;
    event.preventDefault();
    event.stopPropagation();
    releaseConnectionCapture(event);
    setConnectionDrag(null);

    const target = portDropTargetFromPoint(event.clientX, event.clientY);
    if (!target || target.direction !== 'input') {
      setConnectionStatus('未连接：请选择输入端口');
      return true;
    }
    if (!portTypeMatches(current.source.type, target.type)) {
      setConnectionStatus('端口类型不兼容');
      return true;
    }

    const existingEdge = findInputConnection(drawGraph.edges, target);
    const proposal = buildConnectionProposalInput({
      baseVersionId: versionId,
      source: current.source,
      target,
      existingEdge,
    });
    if (!proposal) {
      setConnectionStatus('连线未变化');
      return true;
    }
    if (existingEdge && proposal.ops.length > 1 && !confirmReplace(target)) {
      setConnectionStatus('已取消替换');
      return true;
    }

    void onCreateProposal?.(proposal)
      .then(() =>
        setConnectionStatus(existingEdge ? '已加入编辑会话：替换连线' : '已加入编辑会话：连接端口'),
      )
      .catch((error) => {
        setConnectionStatus(error instanceof Error ? error.message : '连线提交失败');
      });
    return true;
  };

  const disconnectEdge = (edge: WorkbenchState['graph']['edges'][number]) => {
    if (!onCreateProposal || connectionDisabled) return;
    void onCreateProposal({
      baseVersionId: versionId,
      label: `断开 ${edge.from.nodeId}.${edge.from.port} -> ${edge.to.nodeId}.${edge.to.port}`,
      ops: [edgeToRemoveOp(edge)],
    })
      .then(() => setConnectionStatus('已加入编辑会话：断开连线'))
      .catch((error) => {
        setConnectionStatus(error instanceof Error ? error.message : '断线提交失败');
      });
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
    const moved = moveNodeDrafts(currentDrag.starts, {
      x: (event.clientX - currentDrag.sx) / view.z,
      y: (event.clientY - currentDrag.sy) / view.z,
    });
    const updates = positionUpdatesFromDrafts(graph.nodes, moved);
    const editInput = buildMoveNodeEditInput(versionId, updates);
    if (editInput && onCreateProposal && !pendingProposal) {
      void onCreateProposal(editInput)
        .then(() => {
          setDraftPositions({});
          setConnectionStatus(`已加入编辑会话 · ${updates.length} 个移动`);
        })
        .catch((error) => {
          setConnectionStatus(error instanceof Error ? error.message : '移动节点失败');
        });
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
        setConnectionStatus(null);
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
        drag.current = {
          pointerId: event.pointerId,
          sx: event.clientX,
          sy: event.clientY,
          ox: view.x,
          oy: view.y,
        };
      }}
      onPointerMove={(event) => {
        if (connectionDrag?.pointerId === event.pointerId) {
          event.preventDefault();
          event.stopPropagation();
          setConnectionDrag((current) =>
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
        if (cancelConnectionDrag(event)) return;
        if (completeSelectionDrag(event)) return;
        stopDrag(event);
      }}
      onPointerUp={(event) => {
        if (completeConnectionDrag(event)) return;
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
        {connectionStatus && <span className="canvas-pill">{connectionStatus}</span>}
        {!pendingProposal && hasDirtyLayout && (
          <button
            className="layout-save"
            disabled={layoutSaving || !onSaveLayout}
            onClick={() => void saveLayout()}
          >
            {layoutSaving ? '保存中' : `保存布局 · ${layoutUpdates.length}`}
          </button>
        )}
        {clipboardStatus && <span className="canvas-pill">{clipboardStatus}</span>}
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
          onDisconnectEdge={connectionDisabled ? undefined : disconnectEdge}
        />
        {connectionDrag && (
          <svg className="edge-svg connection-drag-svg">
            <path
              className="connection-drag-path"
              d={connectionPath(connectionDrag.sourcePoint, connectionDrag.currentPoint)}
              fill="none"
              stroke={portColor(connectionDrag.source.type)}
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
            portHighlights={portHighlights}
            selected={selectedIds.has(node.id)}
            stepState={stepStateByNodeId.get(node.id) ?? node.status}
            onOutputPortPointerDown={startConnectionDrag}
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
      {selectionDrag && (
        <span
          className="selection-rect"
          style={selectionRectFromPoints(selectionDrag.start, selectionDrag.current)}
        />
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
      {selectedNodes.length > 1 && (
        <GraphSelectionInspector nodes={selectedNodes} onClose={() => setSelectedIds(new Set())} />
      )}
      {selectedNodes.length === 1 && selectedNode && (
        <GraphInspector
          catalogError={catalogError}
          definition={definitionByType.get(selectedNode.nodeType)}
          node={selectedNode}
          onClose={() => setSelectedIds(new Set())}
          onRequestProposal={pendingProposal ? undefined : onRequestNodeProposal}
          onSetParam={pendingProposal ? undefined : onSetParam}
          workflowNode={selectedWorkflowNode}
        />
      )}
    </section>
  );
}
