import { useEffect, useMemo, useState } from 'react';
import { fetchNodeCatalog } from '../api';
import { Icon } from '../icons';
import type {
  ManualProposalInput,
  NodeCatalog,
  WorkflowGraph,
  WorkbenchState,
} from '../types';
import {
  buildManualProposalInput,
  defaultParamsForDefinition,
  type ManualProposalOperation,
} from './manual-proposal-helpers';

export {
  buildManualProposalInput,
  defaultParamsForDefinition,
  parseManualJson,
} from './manual-proposal-helpers';

type Operation = ManualProposalOperation;

type ManualProposalPanelProps = {
  busy: boolean;
  state: WorkbenchState;
  initialCatalog?: NodeCatalog;
  onCreateProposal: (input: ManualProposalInput) => Promise<void>;
};

export function ManualProposalPanel({
  busy,
  state,
  initialCatalog,
  onCreateProposal,
}: ManualProposalPanelProps) {
  const [catalog, setCatalog] = useState<NodeCatalog | null>(initialCatalog ?? null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [operation, setOperation] = useState<Operation>('set_param');
  const [nodeType, setNodeType] = useState('input.text');
  const [nodeId, setNodeId] = useState('manual_text');
  const [nodeTitle, setNodeTitle] = useState('');
  const [paramsText, setParamsText] = useState('{"text": "manual input"}');
  const [x, setX] = useState('120');
  const [y, setY] = useState('320');
  const [targetNodeId, setTargetNodeId] = useState(state.graph.nodes[0]?.id ?? '');
  const [paramKey, setParamKey] = useState('duration_sec');
  const [paramValue, setParamValue] = useState('4');
  const [fromNode, setFromNode] = useState(state.graph.nodes[0]?.id ?? '');
  const [spawnFrom, setSpawnFrom] = useState('');
  const [fromPort, setFromPort] = useState('text');
  const [toNode, setToNode] = useState(state.graph.nodes[1]?.id ?? '');
  const [toPort, setToPort] = useState('prompt');
  const [edgeType, setEdgeType] = useState('text');
  const [edgeIndex, setEdgeIndex] = useState('0');
  const [localError, setLocalError] = useState<string | null>(null);
  const workflowGraph = state.workflowGraph;
  const disabled = busy || !workflowGraph || Boolean(state.pendingProposal);
  const nodeDefinitions = catalog?.nodes ?? [];
  const selectedDefinition = nodeDefinitions.find((definition) => definition.type === nodeType);
  const workflowNodes = useMemo(() => Object.entries(workflowGraph?.nodes ?? {}), [workflowGraph]);
  const nodeById = useMemo(() => new Map(workflowNodes), [workflowNodes]);
  const definitionByType = useMemo(
    () => new Map(nodeDefinitions.map((definition) => [definition.type, definition])),
    [nodeDefinitions],
  );
  const fromDefinition = fromNode ? definitionByType.get(nodeById.get(fromNode)?.node_type ?? '') : null;
  const toDefinition = toNode ? definitionByType.get(nodeById.get(toNode)?.node_type ?? '') : null;

  useEffect(() => {
    if (initialCatalog) return;
    let cancelled = false;
    fetchNodeCatalog()
      .then((next) => {
        if (cancelled) return;
        setCatalog(next);
        setCatalogError(null);
        if (next.nodes[0]) {
          setNodeType(next.nodes[0].type);
          setParamsText(JSON.stringify(defaultParamsForDefinition(next.nodes[0]), null, 2));
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
  }, [initialCatalog]);

  useEffect(() => {
    if (!selectedDefinition) return;
    setParamsText(JSON.stringify(defaultParamsForDefinition(selectedDefinition), null, 2));
  }, [selectedDefinition]);

  const submit = async () => {
    if (disabled) return;
    try {
      setLocalError(null);
      await onCreateProposal(
        buildManualProposalInput({
          baseVersionId: state.workspace.versionId,
          edgeIndex,
          edgeType,
          fromNode: operation === 'spawn_node' ? spawnFrom : fromNode,
          fromPort,
          nodeId,
          nodeTitle,
          nodeType,
          operation,
          paramKey,
          paramValue,
          paramsText,
          targetNodeId,
          toNode,
          toPort,
          workflowGraph,
          x,
          y,
        }),
      );
    } catch (error) {
      setLocalError(error instanceof Error ? error.message : 'manual edit request failed');
    }
  };

  return (
    <section className="manual-proposal-panel">
      <div className="manual-proposal-head">
        <span>
          <Icon n="layers" s={13} />
          Advanced edit
        </span>
        {state.pendingProposal && <em>agent pending</em>}
      </div>
      <div className="manual-grid">
        <label>
          op
          <select
            disabled={busy}
            value={operation}
            onChange={(event) => setOperation(event.target.value as Operation)}
          >
            <option value="set_param">edit param</option>
            <option value="spawn_node">spawn node</option>
            <option value="remove_node">remove node</option>
            <option value="add_edge">connect edge</option>
            <option value="remove_edge">disconnect edge</option>
          </select>
        </label>
        {operation === 'spawn_node' && (
          <>
            <label>
              type
              <select
                disabled={busy || nodeDefinitions.length === 0}
                value={nodeType}
                onChange={(event) => setNodeType(event.target.value)}
              >
                {nodeDefinitions.map((definition) => (
                  <option key={definition.type} value={definition.type}>
                    {definition.title}
                  </option>
                ))}
              </select>
            </label>
            <label>
              id
              <input disabled={busy} value={nodeId} onChange={(event) => setNodeId(event.target.value)} />
            </label>
            <label>
              from
              <input
                disabled={busy}
                placeholder="source node id"
                value={spawnFrom}
                onChange={(event) => setSpawnFrom(event.target.value)}
              />
            </label>
            <label>
              title
              <input
                disabled={busy}
                placeholder={selectedDefinition?.title ?? 'Node title'}
                value={nodeTitle}
                onChange={(event) => setNodeTitle(event.target.value)}
              />
            </label>
            <label>
              x
              <input disabled={busy} value={x} onChange={(event) => setX(event.target.value)} />
            </label>
            <label>
              y
              <input disabled={busy} value={y} onChange={(event) => setY(event.target.value)} />
            </label>
            <label className="manual-span">
              params JSON
              <textarea
                disabled={busy}
                rows={3}
                value={paramsText}
                onChange={(event) => setParamsText(event.target.value)}
              />
            </label>
          </>
        )}
        {(operation === 'remove_node' || operation === 'set_param') && (
          <NodeSelect
            disabled={busy}
            label="node"
            nodes={workflowNodes}
            value={targetNodeId}
            onChange={setTargetNodeId}
          />
        )}
        {operation === 'set_param' && (
          <>
            <label>
              key
              <input disabled={busy} value={paramKey} onChange={(event) => setParamKey(event.target.value)} />
            </label>
            <label className="manual-span">
              value JSON
              <textarea
                disabled={busy}
                rows={2}
                value={paramValue}
                onChange={(event) => setParamValue(event.target.value)}
              />
            </label>
          </>
        )}
        {operation === 'add_edge' && (
          <>
            <NodeSelect disabled={busy} label="from" nodes={workflowNodes} value={fromNode} onChange={setFromNode} />
            <PortSelect
              disabled={busy}
              label="out"
              ports={fromDefinition?.outputs.map((port) => port.name) ?? []}
              value={fromPort}
              onChange={setFromPort}
            />
            <NodeSelect disabled={busy} label="to" nodes={workflowNodes} value={toNode} onChange={setToNode} />
            <PortSelect
              disabled={busy}
              label="in"
              ports={toDefinition?.inputs.map((port) => port.name) ?? []}
              value={toPort}
              onChange={setToPort}
            />
            <label>
              edge
              <input disabled={busy} value={edgeType} onChange={(event) => setEdgeType(event.target.value)} />
            </label>
          </>
        )}
        {operation === 'remove_edge' && (
          <label className="manual-span">
            edge
            <select disabled={busy} value={edgeIndex} onChange={(event) => setEdgeIndex(event.target.value)}>
              {(workflowGraph?.edges ?? []).map((edge, index) => (
                <option key={`${edge.from.join('.')}-${edge.to.join('.')}-${index}`} value={String(index)}>
                  {edge.from.join('.')}{' -> '}{edge.to.join('.')} · {edge.edge_type}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      {(localError || catalogError || !workflowGraph) && (
        <div className="manual-error">{localError ?? catalogError ?? 'Current workflow graph is unavailable.'}</div>
      )}
      <button className="btn btn--soft btn--sm manual-submit" disabled={disabled} onClick={() => void submit()}>
        <Icon n="spark" s={13} fill />
        Add to edit session
      </button>
    </section>
  );
}

function NodeSelect({
  disabled,
  label,
  nodes,
  value,
  onChange,
}: {
  disabled: boolean;
  label: string;
  nodes: Array<[string, WorkflowGraph['nodes'][string]]>;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label>
      {label}
      <select disabled={disabled} value={value} onChange={(event) => onChange(event.target.value)}>
        {nodes.map(([id, node]) => (
          <option key={id} value={id}>
            {id} · {node.title}
          </option>
        ))}
      </select>
    </label>
  );
}

function PortSelect({
  disabled,
  label,
  ports,
  value,
  onChange,
}: {
  disabled: boolean;
  label: string;
  ports: string[];
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label>
      {label}
      <select disabled={disabled} value={value} onChange={(event) => onChange(event.target.value)}>
        {ports.length === 0 ? <option value={value}>{value}</option> : null}
        {ports.map((port) => (
          <option key={port} value={port}>
            {port}
          </option>
        ))}
      </select>
    </label>
  );
}
