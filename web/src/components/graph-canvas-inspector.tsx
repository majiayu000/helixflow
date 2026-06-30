import type { GraphNodeState } from '../types';
import { categorySwatch, paramsFromSummary } from './graph-canvas-rendering';

export function GraphInspector({ node, onClose }: { node: GraphNodeState; onClose: () => void }) {
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

export function GraphSelectionInspector({
  nodes,
  onClose,
}: {
  nodes: GraphNodeState[];
  onClose: () => void;
}) {
  const categories = [...new Set(nodes.map((node) => node.category))].sort();
  return (
    <div className="inspector p-inspector" onClick={(event) => event.stopPropagation()}>
      <div className="inspector-head">
        <div className="kicker">多选 · {nodes.length} 个节点</div>
        <div className="title">
          <span />
          Selection summary
        </div>
        <button className="p-close" onClick={onClose}>
          x
        </button>
      </div>
      <div className="inspector-body">
        <div className="field">
          <span className="field-label">categories</span>
          <span className="field-input">{categories.join(', ')}</span>
        </div>
        <div className="selection-summary-list">
          {nodes.map((node) => (
            <div className="selection-summary-row" key={node.id}>
              <span style={{ background: categorySwatch(node.category) }} />
              <div>
                <strong>{node.title}</strong>
                <small>{node.id}</small>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
