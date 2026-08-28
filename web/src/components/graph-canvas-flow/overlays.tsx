import type {
  ImplementationResolution,
  ModelCatalog,
  NodeCatalog,
  NodeDefinition,
  WorkbenchState,
} from '../../types';
import { CanvasCommentsPanel } from '../graph-canvas-collaboration';
import type { createCanvasEditActions } from '../graph-canvas-edit-actions';
import { GraphInspector, GraphSelectionInspector } from '../graph-canvas-inspector';
import type { ViewState, ViewportSize } from '../graph-canvas-navigation';
import { EmptyCanvas } from '../graph-canvas-overlays';
import type { GraphCanvasProps } from '../graph-canvas-types';
import { NodeLibrary } from '../node-library';
import { CanvasStatusToast } from './controls';

type Readiness = NonNullable<WorkbenchState['providers']['capabilityReadiness']>[number];
type EditActions = ReturnType<typeof createCanvasEditActions>;

type CanvasOverlaysProps = Pick<
  GraphCanvasProps,
  'comments' | 'onCommentOp' | 'onRequestNodeProposal' | 'onSetParam' | 'onStartFromPrompt' | 'providers'
> & {
  canEdit: boolean;
  catalog: NodeCatalog | null;
  catalogError: string | null;
  definitionByType: Map<string, NodeDefinition>;
  drawGraph: WorkbenchState['graph'];
  editActions: EditActions;
  editStatus: string | null;
  modelCatalog: ModelCatalog | null;
  modelCatalogError: string | null;
  pendingProposal: boolean;
  readiness?: Readiness;
  resolution: ImplementationResolution | null;
  selectedIds: Set<string>;
  selectedNodes: WorkbenchState['graph']['nodes'];
  setSelection: (ids: Iterable<string>) => void;
  view: ViewState;
  viewportSize: ViewportSize;
  workflowGraph?: WorkbenchState['workflowGraph'];
};

export function CanvasOverlays(props: CanvasOverlaysProps) {
  const selectedNode = props.selectedNodes.length === 1 ? props.selectedNodes[0] : undefined;
  const selectedNodeId = selectedNode?.id;
  return (
    <>
      <NodeLibrary
        catalog={props.catalog}
        disabled={!props.canEdit}
        error={props.catalogError}
        modelCatalog={props.modelCatalog}
        modelCatalogError={props.modelCatalogError}
        providers={props.providers}
        onAddNode={props.editActions.addNode}
      />
      <CanvasStatusToast
        editStatus={props.editStatus}
        pendingProposal={props.pendingProposal}
      />
      {props.drawGraph.nodes.length === 0 && (
        <EmptyCanvas disabled={!props.canEdit} onPrompt={props.onStartFromPrompt} />
      )}
      {props.selectedNodes.length > 1 && (
        <GraphSelectionInspector
          nodes={props.selectedNodes}
          onClose={() => props.setSelection([])}
          onCopy={() => void props.editActions.copySelection(props.selectedNodes)}
          onDelete={() => props.editActions.deleteSelection(props.selectedIds)}
          view={props.view}
          viewportSize={props.viewportSize}
        />
      )}
      {selectedNode && (
        <GraphInspector
          catalogError={props.catalogError}
          definition={props.definitionByType.get(selectedNode.nodeType)}
          node={selectedNode}
          resolution={props.resolution}
          readiness={props.readiness}
          onClose={() => props.setSelection([])}
          onRequestProposal={props.canEdit ? props.onRequestNodeProposal : undefined}
          onSetParam={props.canEdit ? props.onSetParam : undefined}
          view={props.view}
          viewportSize={props.viewportSize}
          workflowNode={props.workflowGraph?.nodes[selectedNode.id]}
        />
      )}
      <CanvasCommentsPanel
        comments={props.comments ?? []}
        edges={props.drawGraph.edges}
        nodes={props.drawGraph.nodes}
        onCommentOp={props.canEdit ? props.onCommentOp : undefined}
        selectedNodeId={selectedNodeId}
        view={props.view}
        viewportSize={props.viewportSize}
      />
    </>
  );
}
