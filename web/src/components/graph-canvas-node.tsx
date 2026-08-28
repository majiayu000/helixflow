import type { CSSProperties, KeyboardEvent, PointerEvent } from 'react';
import { Icon, Port } from '../icons';
import type { GraphNodeState, NodeDefinition, RunStepState, WorkflowGraph } from '../types';
import type { CanvasNodeArtifact } from './graph-canvas-artifacts';
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
  workflowNode?: WorkflowGraph['nodes'][string];
  diffState: DiffState;
  dirty: boolean;
  locked: boolean;
  connectionDisabled: boolean;
  portHighlights: Map<string, PortHighlight>;
  selected: boolean;
  stepState: RunStepState;
  artifactOutputs: CanvasNodeArtifact[];
  resizable: boolean;
  embedded?: boolean;
  onSelectOutput?: (outputId: string) => void;
  onKeyboardSelect?: (additive: boolean) => void;
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
  workflowNode,
  diffState,
  dirty,
  locked,
  connectionDisabled,
  portHighlights,
  selected,
  stepState,
  artifactOutputs,
  resizable,
  embedded = false,
  onSelectOutput,
  onKeyboardSelect,
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
  const primaryTextParam = selected ? primaryNodeTextParam(workflowNode?.params) : null;
  const classes = [
    'node',
    diffState === 'add' ? 'node--add' : '',
    diffState === 'upd' ? 'node--upd' : '',
    dirty ? 'node--dirty' : '',
    locked ? 'node--locked' : '',
    embedded ? 'node--embedded' : '',
    selected ? 'p-sel' : '',
    active ? 'p-active' : '',
    failed ? 'node--err' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div
      aria-label={`${node.title} (${node.nodeType})`}
      aria-current={selected ? 'true' : undefined}
      className={classes}
      onKeyDown={(event: KeyboardEvent<HTMLDivElement>) => {
        if (event.key !== 'Enter' && event.key !== ' ') return;
        event.preventDefault();
        event.stopPropagation();
        onKeyboardSelect?.(event.shiftKey || event.metaKey || event.ctrlKey);
      }}
      onClick={embedded ? undefined : (event) => event.stopPropagation()}
      onPointerCancel={onPointerCancel}
      onPointerDown={(event) => {
        if (isNodeInteractiveTarget(event.target)) {
          event.stopPropagation();
          return;
        }
        onPointerDown(event);
      }}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      role="group"
      style={{
        left: embedded ? undefined : node.position.x,
        top: embedded ? undefined : node.position.y,
        width: graphNodeWidth(node),
        minHeight: graphNodeHeight(node),
        '--swatch': categorySwatch(node.category),
      } as CSSProperties}
      tabIndex={embedded ? -1 : 0}
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
      {selected && <span className="node-flag selected">SELECTED</span>}
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
        {primaryTextParam && (
          <div className="node-inline-editor">
            <span className="node-inline-label">{primaryTextParam.key}</span>
            <pre>{primaryTextParam.value}</pre>
          </div>
        )}
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
        {artifactOutputs.length > 0 && (
          <div className="node-artifacts">
            {artifactOutputs.slice(0, 1).map((output) => (
              <button
                className={output.selected ? 'node-artifact node-artifact--selected' : 'node-artifact'}
                key={output.id}
                onClick={(event) => {
                  event.stopPropagation();
                  onSelectOutput?.(output.id);
                }}
                title={output.meta || output.title}
                type="button"
              >
                {output.preview?.kind === 'image' ? (
                  <img alt="" className="node-artifact-preview" src={output.preview.content} />
                ) : (
                  <span className="node-artifact-icon"><Icon n={artifactIcon(output.kind)} s={12} /></span>
                )}
                <span className="node-artifact-copy">
                  <strong>{output.title}</strong>
                  <small>{output.kind}</small>
                </span>
              </button>
            ))}
            {artifactOutputs.length > 1 && (
              <span className="node-artifact-more">+{artifactOutputs.length - 1} 个结果</span>
            )}
          </div>
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

function primaryNodeTextParam(params: unknown): { key: string; value: string } | null {
  if (!params || typeof params !== 'object' || Array.isArray(params)) return null;
  const entries = Object.entries(params as Record<string, unknown>);
  const preferred = ['brief', 'prompt', 'caption', 'text', 'style', 'directive'];
  const found = preferred
    .map((key) => entries.find(([entryKey]) => entryKey.toLowerCase() === key))
    .find((entry): entry is [string, unknown] => Boolean(entry));
  const fallback = found ?? entries.find(([, value]) => typeof value === 'string');
  if (!fallback) return null;
  const [key, value] = fallback;
  if (typeof value !== 'string' && typeof value !== 'number') return null;
  const text = String(value).trim();
  return text.length > 0 ? { key, value: text } : null;
}

function isNodeInteractiveTarget(target: EventTarget | null): boolean {
  const candidate = target as (EventTarget & { closest?: (selector: string) => Element | null }) | null;
  if (!candidate) return false;
  return typeof candidate.closest === 'function' &&
    Boolean(candidate.closest('button, input, textarea, select, a'));
}

function artifactIcon(kind: string): 'export' | 'image' | 'layers' | 'play' {
  if (kind === 'image') return 'image';
  if (kind === 'video') return 'play';
  if (kind === 'html' || kind === 'markdown' || kind === 'text' || kind === 'json') {
    return 'export';
  }
  return 'layers';
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
