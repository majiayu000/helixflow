import { portColor } from '../icons';
import type { GraphNodeState, WorkbenchState } from '../types';
import { edgePath, edgeSignature } from './graph-canvas-rendering';

type GraphEdgesProps = {
  edges: WorkbenchState['graph']['edges'];
  nodeById: Map<string, GraphNodeState>;
  baseEdgeIds: Set<string>;
  hasProposal: boolean;
};

export function GraphEdges({ edges, nodeById, baseEdgeIds, hasProposal }: GraphEdgesProps) {
  return (
    <svg className="edge-svg">
      {edges.map((edge) => {
        const from = nodeById.get(edge.from.nodeId);
        const to = nodeById.get(edge.to.nodeId);
        if (!from || !to) return null;
        const isNew = hasProposal ? !baseEdgeIds.has(edgeSignature(edge)) : false;
        return (
          <path
            className={isNew ? 'edge-path edge-path--new' : 'edge-path'}
            d={edgePath(from, to)}
            fill="none"
            key={edge.id}
            stroke={portColor(edge.kind)}
            strokeLinecap="round"
            strokeWidth="3"
          />
        );
      })}
    </svg>
  );
}
