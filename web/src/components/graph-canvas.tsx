import { useMemo, useRef, useState, type CSSProperties, type PointerEvent } from 'react';
import { Icon, Port, portColor } from '../icons';
import type { GraphNodeState, RunStepState, WorkbenchState } from '../types';

type GraphCanvasProps = {
  graph: WorkbenchState['graph'];
  pendingProposal: WorkbenchState['pendingProposal'];
  run: NonNullable<WorkbenchState['run']>;
};

type ViewState = {
  x: number;
  y: number;
  z: number;
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

const nodeWidth = 188;
const headHeight = 31;
const rowHeight = 26;

export function GraphCanvas({ graph, pendingProposal, run }: GraphCanvasProps) {
  const [view, setView] = useState<ViewState>({ x: 20, y: 18, z: 0.78 });
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
        <button onClick={() => setView((current) => ({ ...current, z: Math.max(0.4, current.z - 0.1) }))}>
          -
        </button>
        <span>{Math.round(view.z * 100)}%</span>
        <button onClick={() => setView((current) => ({ ...current, z: Math.min(1.4, current.z + 0.1) }))}>
          +
        </button>
      </div>
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
