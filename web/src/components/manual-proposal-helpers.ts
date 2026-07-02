import type { ManualProposalInput, NodeDefinition, WorkflowGraph } from '../types';

export type ManualProposalOperation =
  | 'add_node'
  | 'remove_node'
  | 'set_param'
  | 'add_edge'
  | 'remove_edge';

type ManualFormInput = {
  baseVersionId: string;
  edgeIndex: string;
  edgeType: string;
  fromNode: string;
  fromPort: string;
  nodeId: string;
  nodeTitle: string;
  nodeType: string;
  operation: ManualProposalOperation;
  paramKey: string;
  paramValue: string;
  paramsText: string;
  targetNodeId: string;
  toNode: string;
  toPort: string;
  workflowGraph?: WorkflowGraph;
  x: string;
  y: string;
};

export function buildManualProposalInput(input: ManualFormInput): ManualProposalInput {
  const label = `Manual ${input.operation.replace('_', ' ')}`;
  if (!input.workflowGraph) {
    throw new Error('Current workflow graph is unavailable.');
  }
  if (input.operation === 'add_node') {
    return {
      baseVersionId: input.baseVersionId,
      label,
      ops: [{
        op: 'add_node',
        id: requiredValue(input.nodeId, 'node id'),
        node_type: requiredValue(input.nodeType, 'node type'),
        title: input.nodeTitle.trim() || undefined,
        params: parseManualJson(input.paramsText),
        pos: [finiteNumber(input.x, 'x'), finiteNumber(input.y, 'y')],
      }],
    };
  }
  if (input.operation === 'remove_node') {
    return {
      baseVersionId: input.baseVersionId,
      label,
      ops: [{ op: 'remove_node', id: requiredValue(input.targetNodeId, 'node id') }],
    };
  }
  if (input.operation === 'set_param') {
    return {
      baseVersionId: input.baseVersionId,
      label,
      ops: [{
        op: 'set_param',
        id: requiredValue(input.targetNodeId, 'node id'),
        key: requiredValue(input.paramKey, 'param key'),
        value: parseManualJson(input.paramValue),
      }],
    };
  }
  if (input.operation === 'add_edge') {
    return {
      baseVersionId: input.baseVersionId,
      label,
      ops: [{
        op: 'add_edge',
        from: [requiredValue(input.fromNode, 'from node'), requiredValue(input.fromPort, 'from port')],
        to: [requiredValue(input.toNode, 'to node'), requiredValue(input.toPort, 'to port')],
        edge_type: requiredValue(input.edgeType, 'edge type'),
      }],
    };
  }
  const edge = input.workflowGraph.edges[Number(input.edgeIndex)];
  if (!edge) {
    throw new Error('edge is required');
  }
  return {
    baseVersionId: input.baseVersionId,
    label,
    ops: [{ op: 'remove_edge', from: edge.from, to: edge.to, edge_type: edge.edge_type }],
  };
}

export function parseManualJson(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch (error) {
    throw new Error(error instanceof Error ? `Invalid JSON: ${error.message}` : 'Invalid JSON');
  }
}

export function defaultParamsForDefinition(definition: NodeDefinition): Record<string, unknown> {
  return Object.fromEntries(
    definition.params_schema.required.map((key) => [
      key,
      defaultValueForParam(definition.params_schema.properties[key]),
    ]),
  );
}

function defaultValueForParam(
  spec: NodeDefinition['params_schema']['properties'][string] | undefined,
): unknown {
  if (spec?.enum_values[0] !== undefined) return spec.enum_values[0];
  if (spec?.type === 'integer' || spec?.type === 'number') return 1;
  if (spec?.type === 'boolean') return false;
  return '';
}

function requiredValue(value: string, label: string): string {
  const trimmed = value.trim();
  if (!trimmed) {
    throw new Error(`${label} is required`);
  }
  return trimmed;
}

function finiteNumber(value: string, label: string): number {
  const number = Number(value);
  if (!Number.isFinite(number)) {
    throw new Error(`${label} must be a finite number`);
  }
  return number;
}
