import type { CSSProperties, PointerEvent } from 'react';
import { Icon, Port } from '../icons';
import type { GraphNodeState, RunStepState } from '../types';
import { GRAPH_NODE_WIDTH } from './graph-canvas-navigation';
import {
  categorySwatch,
  paramsFromSummary,
  type DiffState,
} from './graph-canvas-rendering';

type WorkflowNodeProps = {
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
};

export function WorkflowNode({
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
}: WorkflowNodeProps) {
  const params = paramsFromSummary(node.summary);
  const active = stepState === 'running';
  const done = stepState === 'succeeded';
  const failed = stepState === 'failed';
  const cached = done && node.cached;
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
      {cached && <span className="node-flag cache">缓存</span>}
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
