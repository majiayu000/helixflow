import { portColor } from '../icons';
import type { GraphNodeState, WorkbenchState } from '../types';
import { edgePath, edgeSignature } from './graph-canvas-rendering';

type GraphEdgesProps = {
  edges: WorkbenchState['graph']['edges'];
  nodeById: Map<string, GraphNodeState>;
  baseEdgeIds: Set<string>;
  hasProposal: boolean;
  onDisconnectEdge?: (edge: WorkbenchState['graph']['edges'][number]) => void;
};

export function GraphEdges({
  edges,
  nodeById,
  baseEdgeIds,
  hasProposal,
  onDisconnectEdge,
}: GraphEdgesProps) {
  return (
    <svg className={onDisconnectEdge ? 'edge-svg edge-svg--interactive' : 'edge-svg'}>
      {edges.map((edge) => {
        const from = nodeById.get(edge.from.nodeId);
        const to = nodeById.get(edge.to.nodeId);
        if (!from || !to) return null;
        const isNew = hasProposal ? !baseEdgeIds.has(edgeSignature(edge)) : false;
        return (
          <path
            className={[
              'edge-path',
              isNew ? 'edge-path--new' : '',
              onDisconnectEdge ? 'edge-path--interactive' : '',
            ]
              .filter(Boolean)
              .join(' ')}
            d={edgePath(from, to)}
            fill="none"
            key={edge.id}
            onContextMenu={
              onDisconnectEdge
                ? (event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    onDisconnectEdge(edge);
                  }
                : undefined
            }
            stroke={portColor(edge.kind)}
            strokeLinecap="round"
            strokeWidth="3"
          />
        );
      })}
    </svg>
  );
}
