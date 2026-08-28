import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  applyEdgeChanges,
  applyNodeChanges,
  type EdgeChange,
  type NodeChange,
} from '@xyflow/react';
import { reconcileFlowNodes } from './adapter';
import type { WorkflowFlowEdge, WorkflowFlowNode } from './types';

export function useFlowElements(
  adaptedNodes: WorkflowFlowNode[],
  adaptedEdges: WorkflowFlowEdge[],
) {
  const [nodes, setNodes] = useState(adaptedNodes);
  const [edges, setEdges] = useState(adaptedEdges);
  const nodeRevision = useMemo(() => flowNodeRevision(adaptedNodes), [adaptedNodes]);
  const edgeRevision = useMemo(() => flowEdgeRevision(adaptedEdges), [adaptedEdges]);

  useEffect(() => {
    setNodes((current) => reconcileFlowNodes(current, adaptedNodes));
  }, [nodeRevision]);

  useEffect(() => {
    setEdges((current) => {
      const selectedById = new Map(current.map((edge) => [edge.id, edge.selected] as const));
      return adaptedEdges.map((edge) => ({ ...edge, selected: selectedById.get(edge.id) }));
    });
  }, [edgeRevision]);

  const onNodesChange = useCallback((changes: NodeChange<WorkflowFlowNode>[]) => {
    setNodes((current) => applyNodeChanges(changes, current));
  }, []);

  const onEdgesChange = useCallback((changes: EdgeChange<WorkflowFlowEdge>[]) => {
    setEdges((current) => applyEdgeChanges(changes, current));
  }, []);

  const selectedIds = useMemo(
    () => new Set(nodes.filter((node) => node.selected).map((node) => node.id)),
    [nodes],
  );

  const setSelection = useCallback((ids: Iterable<string>) => {
    const selected = new Set(ids);
    setNodes((current) => current.map((node) => ({
      ...node,
      selected: selected.has(node.id),
    })));
    setEdges((current) => current.map((edge) => ({ ...edge, selected: false })));
  }, []);

  return { edges, nodes, onEdgesChange, onNodesChange, selectedIds, setSelection };
}

function flowNodeRevision(nodes: WorkflowFlowNode[]): string {
  return JSON.stringify(nodes.map((node) => ({
    id: node.id,
    position: node.position,
    style: node.style,
    draggable: node.draggable,
    connectable: node.connectable,
    domain: node.data.node,
    definition: node.data.definition,
    workflowNode: node.data.workflowNode,
    artifacts: node.data.artifactOutputs,
    diffState: node.data.diffState,
    locked: node.data.locked,
    resizable: node.data.resizable,
    stepState: node.data.stepState,
  })));
}

function flowEdgeRevision(edges: WorkflowFlowEdge[]): string {
  return JSON.stringify(edges.map((edge) => ({
    id: edge.id,
    source: edge.source,
    sourceHandle: edge.sourceHandle,
    target: edge.target,
    targetHandle: edge.targetHandle,
    animated: edge.animated,
    style: edge.style,
  })));
}
