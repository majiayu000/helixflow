import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent,
  type WheelEvent,
} from 'react';
import { Icon, Port, portColor } from '../icons';
import type {
  GraphNodeState,
  LayoutPositionUpdate,
  RunStepState,
  WorkbenchState,
} from '../types';
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
  GRAPH_NODE_HEAD_HEIGHT,
  GRAPH_NODE_ROW_HEIGHT,
  GRAPH_NODE_WIDTH,
  MINIMAP_HEIGHT,
  MINIMAP_WIDTH,
  computeMinimapLayout,
  loadGraphCanvasView,
  minimapViewportRect,
  normalizeView,
  saveGraphCanvasView,
  viewForMinimapPoint,
  zoomViewAtPoint,
  type MinimapLayout,
  type ViewState,
  type ViewportSize,
} from './graph-canvas-navigation';

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

type Param = {
  key: string;
  value: string;
};

type DiffState = 'add' | 'upd' | null;

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
  const baseNodeById = useMemo(
    () => new Map(graph.nodes.map((node) => [node.id, node] as const)),
    [graph.nodes],
  );
  const nodeById = useMemo(
    () => new Map(displayNodes.map((node) => [node.id, node] as const)),
    [displayNodes],
  );
  const baseEdgeIds = useMemo(
    () => new Set(graph.edges.map((edge) => edgeSignature(edge))),
    [graph.edges],
  );
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
        <svg className="edge-svg">
          {drawGraph.edges.map((edge) => {
            const from = nodeById.get(edge.from.nodeId);
            const to = nodeById.get(edge.to.nodeId);
            if (!from || !to) return null;
            const isNew = pendingProposal ? !baseEdgeIds.has(edgeSignature(edge)) : false;
            return (
              <path
                className={isNew ? 'edge-path edge-path--new' : 'edge-path'}
                d={edgePath(from, to)}
                fill="none"
                key={edge.id}
                stroke={portColor(edge.kind)}
                strokeLinecap="round"
                strokeWidth="3"
              />
            );
          })}
        </svg>
        {displayNodes.map((node) => (
          <WorkflowNode
            key={node.id}
            diffState={nodeDiffState(node, baseNodeById.get(node.id), Boolean(pendingProposal))}
            dirty={Boolean(draftPositions[node.id]) && !pendingProposal}
            locked={Boolean(pendingProposal)}
            node={node}
            selected={selectedIds.has(node.id)}
            stepState={run.steps.find((step) => step.nodeId === node.id)?.state ?? node.status}
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
      {selectedNode && <Inspector node={selectedNode} onClose={() => setSelectedIds(new Set())} />}
    </section>
  );
}

function WorkflowNode({
  node,
  diffState,
  dirty,
  locked,
  selected,
  stepState,
  onPointerCancel,
  onPointerDown,
  onPointerMove,
  onPointerUp,
}: {
  node: GraphNodeState;
  diffState: DiffState;
  dirty: boolean;
  locked: boolean;
  selected: boolean;
  stepState: RunStepState;
  onPointerCancel: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerDown: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerMove: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerUp: (event: PointerEvent<HTMLDivElement>) => void;
}) {
  const params = paramsFromSummary(node.summary);
  const active = stepState === 'running';
  const done = stepState === 'succeeded';
  const failed = stepState === 'failed';
  const classes = [
    'node',
    diffState === 'add' ? 'node--add' : '',
    diffState === 'upd' ? 'node--upd' : '',
    dirty ? 'node--dirty' : '',
    locked ? 'node--locked' : '',
    selected ? 'p-sel' : '',
    active ? 'p-active' : '',
    failed ? 'node--err' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div
      className={classes}
      onClick={(event) => event.stopPropagation()}
      onPointerCancel={onPointerCancel}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      style={{
        left: node.position.x,
        top: node.position.y,
        width: GRAPH_NODE_WIDTH,
        '--swatch': categorySwatch(node.category),
      } as CSSProperties}
    >
      {done && (
        <span className="p-done">
          <Icon n="check" s={10} sw={2.2} />
        </span>
      )}
      {active && <span className="p-spin" />}
      {diffState === 'add' && <span className="node-flag add">+ 新增</span>}
      {diffState === 'upd' && <span className="node-flag upd">~ 修改</span>}
      {failed && <span className="node-flag err">失败</span>}
      <div className="node-title">
        <span className="swatch" />
        {node.title}
        <span className="p-nid">{node.id}</span>
      </div>
      <div className="node-body">
        <div className="io-row">
          <span className="io-in">
            <Port type={node.category} />
            {node.category}
          </span>
          <span className="io-out">
            <Port type={node.nodeType} />
            {node.nodeType.split('.').at(-1) ?? 'out'}
          </span>
        </div>
        {params.length === 0 ? (
          <div className="param-row">
            <span className="param-k">type</span>
            <span className="param-v">{node.nodeType}</span>
          </div>
        ) : (
          params.slice(0, 4).map((param) => (
            <div className="param-row" key={param.key}>
              <span className="param-k">{param.key}</span>
              <span className="param-v">{param.value}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function Inspector({ node, onClose }: { node: GraphNodeState; onClose: () => void }) {
  const params = paramsFromSummary(node.summary);
  return (
    <div className="inspector p-inspector" onClick={(event) => event.stopPropagation()}>
      <div className="inspector-head">
        <div className="kicker">选中节点 · {node.id}</div>
        <div className="title">
          <span style={{ background: categorySwatch(node.category) }} />
          {node.title}
        </div>
        <button className="p-close" onClick={onClose}>
          x
        </button>
      </div>
      <div className="inspector-body">
        <div className="field">
          <span className="field-label">node type</span>
          <span className="field-input">{node.nodeType}</span>
        </div>
        <div className="field">
          <span className="field-label">provider</span>
          <span className="field-input">{node.provider ?? 'local/builtin'}</span>
        </div>
        {params.map((param) => (
          <div className="field" key={param.key}>
            <span className="field-label">{param.key}</span>
            <span className="field-input field-area">{param.value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function CanvasMinimap({
  layout,
  view,
  viewportSize,
  onNavigate,
}: {
  layout: MinimapLayout;
  view: ViewState;
  viewportSize: ViewportSize;
  onNavigate: (x: number, y: number) => void;
}) {
  const [dragging, setDragging] = useState(false);
  const viewportRect = minimapViewportRect(layout, view, viewportSize);

  const navigate = (event: PointerEvent<HTMLDivElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    onNavigate(event.clientX - rect.left, event.clientY - rect.top);
  };

  return (
    <div
      aria-label="Graph minimap"
      className="canvas-minimap"
      onPointerDown={(event) => {
        event.preventDefault();
        event.stopPropagation();
        event.currentTarget.setPointerCapture(event.pointerId);
        setDragging(true);
        navigate(event);
      }}
      onPointerMove={(event) => {
        if (dragging) navigate(event);
      }}
      onPointerUp={() => setDragging(false)}
      onPointerCancel={() => setDragging(false)}
    >
      {layout.nodes.map((node) => (
        <span
          className="canvas-minimap-node"
          key={node.id}
          style={{
            left: node.x,
            top: node.y,
            width: node.width,
            height: node.height,
          }}
        />
      ))}
      <span
        className="canvas-minimap-viewport"
        style={{
          left: viewportRect.x,
          top: viewportRect.y,
          width: viewportRect.width,
          height: viewportRect.height,
        }}
      />
    </div>
  );
}

function nodeDiffState(
  node: GraphNodeState,
  baseNode: GraphNodeState | undefined,
  hasProposal: boolean,
): DiffState {
  if (!hasProposal) return null;
  if (!baseNode) return 'add';
  return comparableNode(node) === comparableNode(baseNode) ? null : 'upd';
}

function comparableNode(node: GraphNodeState): string {
  return JSON.stringify({
    id: node.id,
    nodeType: node.nodeType,
    title: node.title,
    category: node.category,
    position: node.position,
    provider: node.provider,
    summary: node.summary,
  });
}

function edgeSignature(edge: WorkbenchState['graph']['edges'][number]): string {
  return `${edge.from.nodeId}:${edge.from.port}>${edge.to.nodeId}:${edge.to.port}:${edge.kind}`;
}

function edgePath(from: GraphNodeState, to: GraphNodeState): string {
  const x1 = from.position.x + GRAPH_NODE_WIDTH;
  const y1 = from.position.y + GRAPH_NODE_HEAD_HEIGHT + GRAPH_NODE_ROW_HEIGHT;
  const x2 = to.position.x;
  const y2 = to.position.y + GRAPH_NODE_HEAD_HEIGHT + GRAPH_NODE_ROW_HEIGHT;
  const dx = Math.max(40, Math.abs(x2 - x1) * 0.5);
  return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
}

function paramsFromSummary(summary: string): Param[] {
  if (!summary || summary === '{}') return [];
  try {
    const parsed = JSON.parse(summary) as Record<string, unknown>;
    return Object.entries(parsed).map(([key, value]) => ({
      key,
      value: typeof value === 'string' ? value : JSON.stringify(value),
    }));
  } catch {
    return [{ key: 'summary', value: summary }];
  }
}

function categorySwatch(category: string): string {
  const key = category.toLowerCase();
  if (key.includes('input')) return 'var(--t-image)';
  if (key.includes('text')) return 'var(--t-cond)';
  if (key.includes('video')) return 'var(--t-clip)';
  if (key.includes('image')) return 'var(--t-image)';
  if (key.includes('output')) return 'var(--green)';
  if (key.includes('mock')) return 'var(--amber)';
  return 'var(--accent)';
}

function runStatusLabel(status: NonNullable<WorkbenchState['run']>['status']): string {
  if (status === 'running') return '运行中';
  if (status === 'succeeded') return '已完成';
  if (status === 'failed') return '失败';
  if (status === 'interrupted') return '已中断';
  if (status === 'estimating') return '估算中';
  return '未运行';
}
