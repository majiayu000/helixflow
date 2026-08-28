import type { CSSProperties } from 'react';
import type { NodeDefinition, RunStepState, WorkbenchState } from '../../types';
import { portColor } from '../../icons';
import { graphNodeHeight, graphNodeWidth } from '../graph-canvas-navigation';
import { edgeSignature, nodeDiffState } from '../graph-canvas-rendering';
import type { CanvasNodeArtifact } from '../graph-canvas-artifacts';
import type { ResizeCommit, WorkflowFlowEdge, WorkflowFlowNode } from './types';

type NodeAdapterInput = {
  nodes: WorkbenchState['graph']['nodes'];
  baseComparableById: Map<string, string>;
  definitionByType: Map<string, NodeDefinition>;
  outputsByNodeId: Map<string, CanvasNodeArtifact[]>;
  stepStateByNodeId: Map<string, RunStepState>;
  workflowGraph?: WorkbenchState['workflowGraph'];
  hasProposal: boolean;
  canMove: boolean;
  canResize: boolean;
  onResizeCommit: ResizeCommit;
  onSelectOutput?: (outputId: string) => void;
};

export function toWorkflowFlowNodes(input: NodeAdapterInput): WorkflowFlowNode[] {
  return input.nodes.map((node) => {
    const width = graphNodeWidth(node);
    const height = graphNodeHeight(node);
    return {
      id: node.id,
      type: 'workflow',
      position: node.position,
      initialWidth: width,
      initialHeight: height,
      style: { width, minHeight: height } as CSSProperties,
      draggable: input.canMove,
      selectable: true,
      connectable: input.canMove,
      deletable: false,
      ariaLabel: `${node.title} (${node.nodeType})`,
      data: {
        node,
        definition: input.definitionByType.get(node.nodeType),
        workflowNode: input.workflowGraph?.nodes[node.id],
        artifactOutputs: input.outputsByNodeId.get(node.id) ?? [],
        diffState: nodeDiffState(
          node,
          input.baseComparableById.get(node.id),
          input.hasProposal,
        ),
        dirty: false,
        locked: input.hasProposal,
        resizable: input.canResize,
        stepState: input.stepStateByNodeId.get(node.id) ?? node.status,
        onResizeCommit: input.onResizeCommit,
        onSelectOutput: input.onSelectOutput,
      },
    };
  });
}

export function toWorkflowFlowEdges(
  edges: WorkbenchState['graph']['edges'],
  baseEdgeIds: Set<string>,
  hasProposal: boolean,
): WorkflowFlowEdge[] {
  return edges.map((edge) => {
    const signature = edgeSignature(edge);
    const proposed = hasProposal && !baseEdgeIds.has(signature);
    return {
      id: signature,
      type: 'default',
      source: edge.from.nodeId,
      sourceHandle: edge.from.port,
      target: edge.to.nodeId,
      targetHandle: edge.to.port,
      animated: proposed,
      selectable: true,
      deletable: false,
      data: { graphEdge: edge, proposed },
      style: {
        stroke: portColor(edge.kind),
        strokeWidth: proposed ? 2.5 : 2,
        strokeDasharray: proposed ? '7 6' : undefined,
      },
    };
  });
}

export function reconcileFlowNodes(
  current: WorkflowFlowNode[],
  next: WorkflowFlowNode[],
): WorkflowFlowNode[] {
  const currentById = new Map(current.map((node) => [node.id, node] as const));
  return next.map((node) => {
    const previous = currentById.get(node.id);
    return previous ? { ...node, selected: previous.selected } : node;
  });
}
