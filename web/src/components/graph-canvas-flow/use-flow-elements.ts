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

  const resetNodes = useCallback(() => {
    setNodes((current) => reconcileFlowNodes(current, adaptedNodes));
  }, [adaptedNodes]);

  useEffect(() => resetNodes(), [resetNodes]);

  useEffect(() => {
    setEdges((current) => {
      const selectedById = new Map(current.map((edge) => [edge.id, edge.selected] as const));
      return adaptedEdges.map((edge) => ({ ...edge, selected: selectedById.get(edge.id) }));
    });
  }, [adaptedEdges]);

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
    setNodes((current) => {
      let changed = false;
      const next = current.map((node) => {
        const shouldSelect = selected.has(node.id);
        if (Boolean(node.selected) === shouldSelect) return node;
        changed = true;
        return { ...node, selected: shouldSelect };
      });
      return changed ? next : current;
    });
    setEdges((current) => {
      if (!current.some((edge) => edge.selected)) return current;
      return current.map((edge) => ({ ...edge, selected: false }));
    });
  }, []);

  return { edges, nodes, onEdgesChange, onNodesChange, resetNodes, selectedIds, setSelection };
}
