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
import type { GraphNodeState, RunStepState, WorkbenchState } from '../types';

type GraphCanvasProps = {
  workspaceId: string;
  graph: WorkbenchState['graph'];
  pendingProposal: WorkbenchState['pendingProposal'];
  run: NonNullable<WorkbenchState['run']>;
};

export type ViewState = {
  x: number;
  y: number;
  z: number;
};

type ViewportSize = {
  width: number;
  height: number;
};

type DragState = {
  pointerId: number;
  sx: number;
  sy: number;
  ox: number;
  oy: number;
};

type Param = {
  key: string;
  value: string;
};

type DiffState = 'add' | 'upd' | null;

export const DEFAULT_GRAPH_VIEW: ViewState = { x: 20, y: 18, z: 0.78 };
export const GRAPH_CANVAS_VIEW_STORAGE_PREFIX = 'helixflow:graph-canvas-view:';
const minZoom = 0.4;
const maxZoom = 1.4;
const nodeWidth = 188;
const headHeight = 31;
const rowHeight = 26;
const minimapWidth = 188;
const minimapHeight = 124;
const minimapPadding = 260;

export function GraphCanvas({ workspaceId, graph, pendingProposal, run }: GraphCanvasProps) {
  const canvasRef = useRef<HTMLElement | null>(null);
  const [view, setView] = useState<ViewState>(DEFAULT_GRAPH_VIEW);
  const [viewportSize, setViewportSize] = useState<ViewportSize>({ width: 900, height: 640 });
  const [mode, setMode] = useState<'view' | 'edit' | 'review'>('view');
  const [selected, setSelected] = useState<string | null>(null);
  const drag = useRef<DragState | null>(null);
  const drawGraph = pendingProposal?.previewGraph ?? graph;
  const activeMode = pendingProposal ? 'review' : mode;
  const baseNodeById = useMemo(
    () => new Map(graph.nodes.map((node) => [node.id, node] as const)),
    [graph.nodes],
  );
  const nodeById = useMemo(
    () => new Map(drawGraph.nodes.map((node) => [node.id, node] as const)),
    [drawGraph.nodes],
  );
  const baseEdgeIds = useMemo(
    () => new Set(graph.edges.map((edge) => edgeSignature(edge))),
    [graph.edges],
  );
  const selectedNode = selected ? nodeById.get(selected) : null;
  const nodeCount = drawGraph.nodes.length;
  const minimapLayout = useMemo(
    () => computeMinimapLayout(drawGraph.nodes, { width: minimapWidth, height: minimapHeight }),
    [drawGraph.nodes],
  );

  useEffect(() => {
    setView(loadGraphCanvasView(workspaceId));
    setSelected(null);
  }, [workspaceId]);

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

  return (
    <section
      ref={canvasRef}
      className="p-canvas cv-bold"
      onClick={() => setSelected(null)}
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
        {drawGraph.nodes.map((node) => (
          <WorkflowNode
            key={node.id}
            diffState={nodeDiffState(node, baseNodeById.get(node.id), Boolean(pendingProposal))}
            node={node}
            selected={selected === node.id}
            stepState={run.steps.find((step) => step.nodeId === node.id)?.state ?? node.status}
            onSelect={() => setSelected(node.id)}
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
      {selectedNode && <Inspector node={selectedNode} onClose={() => setSelected(null)} />}
    </section>
  );
}

function WorkflowNode({
  node,
  diffState,
  selected,
  stepState,
  onSelect,
}: {
  node: GraphNodeState;
  diffState: DiffState;
  selected: boolean;
  stepState: RunStepState;
  onSelect: () => void;
}) {
  const params = paramsFromSummary(node.summary);
  const active = stepState === 'running';
  const done = stepState === 'succeeded';
  const failed = stepState === 'failed';
  const classes = [
    'node',
    diffState === 'add' ? 'node--add' : '',
    diffState === 'upd' ? 'node--upd' : '',
    selected ? 'p-sel' : '',
    active ? 'p-active' : '',
    failed ? 'node--err' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div
      className={classes}
      onClick={(event) => {
        event.stopPropagation();
        onSelect();
      }}
      style={{
        left: node.position.x,
        top: node.position.y,
        width: nodeWidth,
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

export type MinimapLayout = {
  width: number;
  height: number;
  worldBounds: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
  scale: number;
  offset: {
    x: number;
    y: number;
  };
  nodes: Array<{
    id: string;
    x: number;
    y: number;
    width: number;
    height: number;
  }>;
};

export function viewStorageKey(workspaceId: string): string {
  return `${GRAPH_CANVAS_VIEW_STORAGE_PREFIX}${workspaceId}`;
}

export function loadGraphCanvasView(workspaceId: string): ViewState {
  const storage = safeLocalStorage();
  if (!storage || !workspaceId) return DEFAULT_GRAPH_VIEW;
  try {
    return normalizeView(JSON.parse(storage.getItem(viewStorageKey(workspaceId)) ?? 'null'));
  } catch {
    return DEFAULT_GRAPH_VIEW;
  }
}

export function saveGraphCanvasView(workspaceId: string, view: ViewState): void {
  const storage = safeLocalStorage();
  if (!storage || !workspaceId) return;
  storage.setItem(viewStorageKey(workspaceId), JSON.stringify(normalizeView(view)));
}

export function zoomViewAtPoint(
  view: ViewState,
  input: { deltaY: number; localX: number; localY: number },
): ViewState {
  const current = normalizeView(view);
  const nextZ = clampZoom(current.z * Math.pow(1.1, -input.deltaY / 100));
  const worldX = (input.localX - current.x) / current.z;
  const worldY = (input.localY - current.y) / current.z;
  return normalizeView({
    x: input.localX - worldX * nextZ,
    y: input.localY - worldY * nextZ,
    z: nextZ,
  });
}

export function clampZoom(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_GRAPH_VIEW.z;
  return Math.min(maxZoom, Math.max(minZoom, value));
}

export function normalizeView(value: unknown): ViewState {
  if (!isRecord(value)) return DEFAULT_GRAPH_VIEW;
  const x = typeof value.x === 'number' && Number.isFinite(value.x) ? value.x : DEFAULT_GRAPH_VIEW.x;
  const y = typeof value.y === 'number' && Number.isFinite(value.y) ? value.y : DEFAULT_GRAPH_VIEW.y;
  const z = typeof value.z === 'number' && Number.isFinite(value.z) ? value.z : DEFAULT_GRAPH_VIEW.z;
  return { x, y, z: clampZoom(z) };
}

export function computeMinimapLayout(
  nodes: GraphNodeState[],
  size: ViewportSize = { width: minimapWidth, height: minimapHeight },
): MinimapLayout | null {
  const measured = nodes
    .map((node) => ({
      id: node.id,
      x: node.position.x,
      y: node.position.y,
      width: nodeWidth,
      height: nodeVisualHeight(node),
    }))
    .filter((node) =>
      [node.x, node.y, node.width, node.height].every((item) => Number.isFinite(item)),
    );
  if (measured.length === 0) return null;

  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  measured.forEach((node) => {
    minX = Math.min(minX, node.x);
    minY = Math.min(minY, node.y);
    maxX = Math.max(maxX, node.x + node.width);
    maxY = Math.max(maxY, node.y + node.height);
  });

  const worldBounds = {
    x: minX - minimapPadding,
    y: minY - minimapPadding,
    width: Math.max(1, maxX - minX + minimapPadding * 2),
    height: Math.max(1, maxY - minY + minimapPadding * 2),
  };
  const scale = Math.min(size.width / worldBounds.width, size.height / worldBounds.height);
  const contentWidth = worldBounds.width * scale;
  const contentHeight = worldBounds.height * scale;
  const offset = {
    x: (size.width - contentWidth) / 2,
    y: (size.height - contentHeight) / 2,
  };

  return {
    width: size.width,
    height: size.height,
    worldBounds,
    scale,
    offset,
    nodes: measured.map((node) => ({
      id: node.id,
      x: (node.x - worldBounds.x) * scale + offset.x,
      y: (node.y - worldBounds.y) * scale + offset.y,
      width: Math.max(2, node.width * scale),
      height: Math.max(2, node.height * scale),
    })),
  };
}

export function viewForMinimapPoint(
  layout: MinimapLayout,
  point: { x: number; y: number },
  view: ViewState,
  viewportSize: ViewportSize,
): ViewState {
  const worldX = (point.x - layout.offset.x) / layout.scale + layout.worldBounds.x;
  const worldY = (point.y - layout.offset.y) / layout.scale + layout.worldBounds.y;
  return normalizeView({
    x: viewportSize.width / 2 - worldX * view.z,
    y: viewportSize.height / 2 - worldY * view.z,
    z: view.z,
  });
}

export function minimapViewportRect(
  layout: MinimapLayout,
  view: ViewState,
  viewportSize: ViewportSize,
) {
  const left = -view.x / view.z;
  const top = -view.y / view.z;
  const width = viewportSize.width / view.z;
  const height = viewportSize.height / view.z;
  const x = (left - layout.worldBounds.x) * layout.scale + layout.offset.x;
  const y = (top - layout.worldBounds.y) * layout.scale + layout.offset.y;
  return {
    x,
    y,
    width: Math.max(4, width * layout.scale),
    height: Math.max(4, height * layout.scale),
  };
}

function nodeVisualHeight(node: GraphNodeState): number {
  const params = paramsFromSummary(node.summary);
  return headHeight + rowHeight + Math.max(1, Math.min(4, params.length || 1)) * rowHeight;
}

function safeLocalStorage(): Storage | null {
  try {
    return typeof globalThis.localStorage === 'undefined' ? null : globalThis.localStorage;
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
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
  const x1 = from.position.x + nodeWidth;
  const y1 = from.position.y + headHeight + rowHeight;
  const x2 = to.position.x;
  const y2 = to.position.y + headHeight + rowHeight;
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
