import type { CSSProperties, PointerEvent } from 'react';
import { Icon, Port } from '../icons';
import type { GraphNodeState, NodeDefinition, RunStepState } from '../types';
import { portKey, type PortHighlight } from './graph-canvas-connections';
import { graphNodeHeight, graphNodeWidth } from './graph-canvas-navigation';
import {
  categorySwatch,
  paramsFromSummary,
  type DiffState,
} from './graph-canvas-rendering';

type WorkflowNodeProps = {
  node: GraphNodeState;
  definition?: NodeDefinition;
  diffState: DiffState;
  dirty: boolean;
  locked: boolean;
  connectionDisabled: boolean;
  portHighlights: Map<string, PortHighlight>;
  selected: boolean;
  stepState: RunStepState;
  resizable: boolean;
  onOutputPortPointerDown: (
    node: GraphNodeState,
    port: { name: string; type: string },
    index: number,
    event: PointerEvent<HTMLSpanElement>,
  ) => void;
  onPointerCancel: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerDown: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerMove: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerUp: (event: PointerEvent<HTMLDivElement>) => void;
  onResizePointerCancel: (event: PointerEvent<HTMLSpanElement>) => void;
  onResizePointerDown: (event: PointerEvent<HTMLSpanElement>) => void;
  onResizePointerMove: (event: PointerEvent<HTMLSpanElement>) => void;
  onResizePointerUp: (event: PointerEvent<HTMLSpanElement>) => void;
};

export function WorkflowNode({
  node,
  definition,
  diffState,
  dirty,
  locked,
  connectionDisabled,
  portHighlights,
  selected,
  stepState,
  resizable,
  onOutputPortPointerDown,
  onPointerCancel,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onResizePointerCancel,
  onResizePointerDown,
  onResizePointerMove,
  onResizePointerUp,
}: WorkflowNodeProps) {
  const params = paramsFromSummary(node.summary);
  const active = stepState === 'running';
  const done = stepState === 'succeeded';
  const failed = stepState === 'failed';
  const cached = done && node.cached;
  const inputs = definition?.inputs ?? [];
  const outputs = definition?.outputs ?? [];
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
        width: graphNodeWidth(node),
        minHeight: graphNodeHeight(node),
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
          <span className="port-stack port-stack--input">
            {inputs.length === 0 ? (
              <span className="port-empty">{node.category}</span>
            ) : (
              inputs.map((port, index) => (
                <span
                  className={portTargetClass(
                    'input',
                    portHighlights.get(portKey(node.id, 'input', port.name)),
                    connectionDisabled,
                  )}
                  data-port-direction="input"
                  data-port-index={index}
                  data-port-name={port.name}
                  data-port-node-id={node.id}
                  data-port-type={port.type}
                  key={port.name}
                  title={`${port.name} · ${port.type}`}
                >
                  <Port type={port.type} />
                  <span>{port.name}</span>
                </span>
              ))
            )}
          </span>
          <span className="port-stack port-stack--output">
            {outputs.length === 0 ? (
              <span className="port-empty">{node.nodeType.split('.').at(-1) ?? 'out'}</span>
            ) : (
              outputs.map((port, index) => (
                <span
                  className={portTargetClass(
                    'output',
                    portHighlights.get(portKey(node.id, 'output', port.name)),
                    connectionDisabled,
                  )}
                  data-port-direction="output"
                  data-port-index={index}
                  data-port-name={port.name}
                  data-port-node-id={node.id}
                  data-port-type={port.type}
                  key={port.name}
                  onPointerDown={(event) => {
                    if (!connectionDisabled) {
                      onOutputPortPointerDown(node, port, index, event);
                    }
                  }}
                  title={`${port.name} · ${port.type}`}
                >
                  <span>{port.name}</span>
                  <Port type={port.type} />
                </span>
              ))
            )}
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
      {resizable && (
        <span
          aria-label={`Resize ${node.title}`}
          className="node-resize-handle"
          onPointerCancel={onResizePointerCancel}
          onPointerDown={onResizePointerDown}
          onPointerMove={onResizePointerMove}
          onPointerUp={onResizePointerUp}
          title="Resize node"
        />
      )}
    </div>
  );
}

function portTargetClass(
  direction: 'input' | 'output',
  highlight: PortHighlight | undefined,
  disabled: boolean,
): string {
  return [
    'port-target',
    `port-target--${direction}`,
    disabled ? 'port-target--disabled' : '',
    highlight ? `port-target--${highlight}` : '',
  ]
    .filter(Boolean)
    .join(' ');
}
