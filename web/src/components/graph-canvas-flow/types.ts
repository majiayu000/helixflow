import type { Edge, Node, ReactFlowInstance } from '@xyflow/react';
import type {
  GraphNodeState,
  NodeDefinition,
  RunStepState,
  WorkbenchState,
  WorkflowGraph,
} from '../../types';
import type { CanvasNodeArtifact } from '../graph-canvas-artifacts';
import type { DiffState } from '../graph-canvas-rendering';

export type ResizeCommit = (nodeId: string, width: number, height: number) => void;

export type WorkflowFlowNodeData = {
  node: GraphNodeState;
  definition?: NodeDefinition;
  workflowNode?: WorkflowGraph['nodes'][string];
  artifactOutputs: CanvasNodeArtifact[];
  diffState: DiffState;
  dirty: boolean;
  locked: boolean;
  resizable: boolean;
  stepState: RunStepState;
  onResizeCommit: ResizeCommit;
  onSelectOutput?: (outputId: string) => void;
};

export type WorkflowFlowNode = Node<WorkflowFlowNodeData, 'workflow'>;

export type WorkflowFlowEdgeData = {
  graphEdge: WorkbenchState['graph']['edges'][number];
  proposed: boolean;
};

export type WorkflowFlowEdge = Edge<WorkflowFlowEdgeData, 'default'>;
export type WorkflowFlowInstance = ReactFlowInstance<WorkflowFlowNode, WorkflowFlowEdge>;
