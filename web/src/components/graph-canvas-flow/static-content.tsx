import type { CanvasPresence, WorkbenchState } from '../../types';
import { LOCAL_CANVAS_ACTOR } from '../../canvas-presence';
import { CanvasCollaborationWorld, ConnectedCanvasCollaborationWorld } from '../graph-canvas-collaboration';
import { GraphEdges } from '../graph-canvas-edges';
import { buildNodeMap } from '../graph-canvas-rendering';
import type { ViewState } from '../graph-canvas-navigation';
import { WorkflowNode } from '../graph-canvas-node';
import { CanvasViewControls } from './controls';
import type { WorkflowFlowNode } from './types';

type StaticContentProps = {
  baseEdgeIds: Set<string>;
  comments: Parameters<typeof CanvasCollaborationWorld>[0]['comments'];
  edges: WorkbenchState['graph']['edges'];
  nodes: WorkflowFlowNode[];
  presenceByActor?: Record<string, CanvasPresence>;
  setSelection: (ids: Iterable<string>) => void;
  view: ViewState;
};

export function StaticFlowContent(props: StaticContentProps) {
  const graphNodes = props.nodes.map((item) => item.data.node);
  const nodeById = buildNodeMap(graphNodes);
  return (
    <>
      <div className="canvas-grid" />
      <div className="world" style={{ left: props.view.x, top: props.view.y, transform: `scale(${props.view.z})` }}>
        <GraphEdges
          baseEdgeIds={props.baseEdgeIds}
          edges={props.edges}
          hasProposal={props.nodes.some((node) => node.data.locked)}
          nodeById={nodeById}
        />
        {props.nodes.map((item) => (
          <WorkflowNode
            {...item.data}
            connectionDisabled={!item.connectable}
            key={item.id}
            portHighlights={EMPTY_HIGHLIGHTS}
            selected={Boolean(item.selected)}
            onKeyboardSelect={(additive) => {
              props.setSelection(additive ? [
                ...props.nodes.filter((node) => node.selected).map((node) => node.id),
                item.id,
              ] : [item.id]);
            }}
            onOutputPortPointerDown={ignoreEvent}
            onPointerCancel={ignoreEvent}
            onPointerDown={ignoreEvent}
            onPointerMove={ignoreEvent}
            onPointerUp={ignoreEvent}
            onResizePointerCancel={ignoreEvent}
            onResizePointerDown={ignoreEvent}
            onResizePointerMove={ignoreEvent}
            onResizePointerUp={ignoreEvent}
          />
        ))}
        {props.presenceByActor ? (
          <CanvasCollaborationWorld comments={props.comments} hiddenActorId={LOCAL_CANVAS_ACTOR.actorId} nodes={graphNodes} presenceByActor={props.presenceByActor} />
        ) : (
          <ConnectedCanvasCollaborationWorld comments={props.comments} nodes={graphNodes} />
        )}
      </div>
      <CanvasViewControls instance={null} nodeCount={props.nodes.length} view={props.view} />
    </>
  );
}

const EMPTY_HIGHLIGHTS = new Map();
const ignoreEvent = () => undefined;
