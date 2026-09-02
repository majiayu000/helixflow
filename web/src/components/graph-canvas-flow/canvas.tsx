import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Background,
  BackgroundVariant,
  ReactFlow,
  SelectionMode,
  ViewportPortal,
  type NodeTypes,
} from '@xyflow/react';
import { LOCAL_CANVAS_ACTOR } from '../../canvas-presence';
import {
  CanvasCollaborationWorld,
  ConnectedCanvasCollaborationWorld,
} from '../graph-canvas-collaboration';
import { outputsByCanvasNode } from '../graph-canvas-artifacts';
import { canvasCapabilities } from '../graph-canvas-capabilities';
import { createCanvasEditActions } from '../graph-canvas-edit-actions';
import {
  GRAPH_CANVAS_MAX_ZOOM,
  GRAPH_CANVAS_MIN_ZOOM,
} from '../graph-canvas-navigation';
import {
  buildComparableNodeMap,
  buildEdgeSignatureSet,
  buildRunStepStateMap,
  edgesForViewport,
  nodeIdsForViewport,
} from '../graph-canvas-rendering';
import type { GraphCanvasProps } from '../graph-canvas-types';
import { toWorkflowFlowEdges, toWorkflowFlowNodes } from './adapter';
import { CanvasViewControls } from './controls';
import { ReactFlowWorkflowNode } from './node';
import { CanvasOverlays } from './overlays';
import { CanvasOverview, shouldUseCanvasOverview } from './overview';
import { StaticFlowContent } from './static-content';
import { useCanvasCatalog, useImplementationResolution } from './use-catalog';
import { handleFlowDragOver, handleFlowDrop, useFlowActions } from './use-flow-actions';
import { useFlowElements } from './use-flow-elements';
import { useFlowPresence } from './use-presence';
import { useCanvasShortcuts } from './use-shortcuts';
import { useFlowViewport } from './use-viewport';

const NODE_TYPES: NodeTypes = { workflow: ReactFlowWorkflowNode };

export function GraphCanvas(props: GraphCanvasProps) {
  const [editStatus, setEditStatus] = useState<string | null>(null);
  const onSelectionChangeRef = useRef(props.onSelectionChange);
  const onSelectOutputRef = useRef(props.onSelectOutput);
  const reportedSelectionRef = useRef({ scope: '', signature: '' });
  const resetFlowNodesRef = useRef<() => void>(() => undefined);
  onSelectionChangeRef.current = props.onSelectionChange;
  onSelectOutputRef.current = props.onSelectOutput;
  const selectOutput = useCallback((id: string) => onSelectOutputRef.current?.(id), []);
  const resetRejectedMutation = useCallback(() => resetFlowNodesRef.current(), []);
  const sourceGraph = props.canvasGraph ?? props.graph;
  const drawGraph = props.pendingProposal?.previewGraph ?? sourceGraph;
  const activeMode = props.pendingProposal ? 'review' : props.onCreateProposal ? 'edit' : 'view';
  const capabilities = canvasCapabilities(activeMode, Boolean(props.onCreateProposal));
  const persistedView = props.canvasViewport
    ? { x: props.canvasViewport.x, y: props.canvasViewport.y, z: props.canvasViewport.zoom }
    : null;
  const viewport = useFlowViewport(
    props.workspaceId,
    persistedView,
    (view) => void props.onSaveCanvasSnapshot?.({
      viewport: { x: view.x, y: view.y, zoom: view.z },
    }),
  );
  const catalogState = useCanvasCatalog();
  const baseComparableById = useMemo(
    () => buildComparableNodeMap(sourceGraph.nodes),
    [sourceGraph.nodes],
  );
  const baseEdgeIds = useMemo(() => buildEdgeSignatureSet(sourceGraph.edges), [sourceGraph.edges]);
  const outputsByNodeId = useMemo(() => outputsByCanvasNode(props.outputs), [props.outputs]);
  const stepSignature = props.run.steps.map((step) => `${step.nodeId}:${step.state}`).join('|');
  const stepStateByNodeId = useMemo(() => buildRunStepStateMap(props.run.steps), [stepSignature]);
  const flowActions = useFlowActions({
    versionId: props.versionId,
    nodes: drawGraph.nodes,
    edges: drawGraph.edges,
    definitionByType: catalogState.definitionByType,
    onCreateProposal: props.onCreateProposal,
    onSaveCanvasSnapshot: props.onSaveCanvasSnapshot,
    onMutationRejected: resetRejectedMutation,
    setStatus: setEditStatus,
  });
  const adaptedNodes = useMemo(() => toWorkflowFlowNodes({
    nodes: drawGraph.nodes,
    baseComparableById,
    definitionByType: catalogState.definitionByType,
    outputsByNodeId,
    stepStateByNodeId,
    workflowGraph: props.workflowGraph,
    hasProposal: Boolean(props.pendingProposal),
    canMove: capabilities.move,
    canResize: capabilities.resize,
    onResizeCommit: flowActions.commitResize,
    onSelectOutput: selectOutput,
  }), [
    baseComparableById, capabilities.move, capabilities.resize, catalogState.definitionByType,
    drawGraph.nodes, flowActions.commitResize, outputsByNodeId, selectOutput,
    props.pendingProposal, props.workflowGraph, stepStateByNodeId,
  ]);
  const renderEdges = useMemo(
    () => edgesForViewport(
      drawGraph.nodes,
      drawGraph.edges,
      viewport.sliceView,
      viewport.viewportSize,
    ),
    [drawGraph.edges, drawGraph.nodes, viewport.sliceView, viewport.viewportSize],
  );
  const adaptedEdges = useMemo(() => (
    drawGraph.nodes.every((node) => catalogState.definitionByType.has(node.nodeType))
      ? toWorkflowFlowEdges(renderEdges, baseEdgeIds, Boolean(props.pendingProposal))
      : []
  ), [baseEdgeIds, catalogState.definitionByType, drawGraph.nodes, props.pendingProposal, renderEdges]);
  const elements = useFlowElements(adaptedNodes, adaptedEdges);
  resetFlowNodesRef.current = elements.resetNodes;
  const useOverview = shouldUseCanvasOverview(drawGraph.nodes.length, viewport.sliceView);
  const renderNodeIds = useMemo(
    () => nodeIdsForViewport(
      drawGraph.nodes,
      renderEdges,
      viewport.sliceView,
      viewport.viewportSize,
    ),
    [drawGraph.nodes, renderEdges, viewport.sliceView, viewport.viewportSize],
  );
  const flowNodes = useMemo(
    () => useOverview
      ? []
      : renderNodeIds
      ? elements.nodes.filter((node) => renderNodeIds.has(node.id))
      : elements.nodes,
    [elements.nodes, renderNodeIds, useOverview],
  );
  const flowEdges = useMemo(
    () => useOverview
      ? []
      : renderNodeIds
      ? elements.edges.filter(
        (edge) => renderNodeIds.has(edge.source) && renderNodeIds.has(edge.target),
      )
      : elements.edges,
    [elements.edges, renderNodeIds, useOverview],
  );
  const selectedNodes = useMemo(
    () => drawGraph.nodes.filter((node) => elements.selectedIds.has(node.id)),
    [drawGraph.nodes, elements.selectedIds],
  );
  const selectedCapability = selectedNodes.length === 1
    ? (catalogState.definitionByType.get(selectedNodes[0]!.nodeType)?.capability ?? null)
    : null;
  const implementation = useImplementationResolution(
    props.workspaceId,
    props.providers,
    selectedCapability,
  );
  const editActions = createCanvasEditActions({
    capabilities,
    drawGraph,
    onCreateProposal: props.onCreateProposal,
    setClipboardStatus: setEditStatus,
    versionId: props.versionId,
    view: viewport.view,
    viewportSize: viewport.viewportSize,
    workflowGraph: props.workflowGraph,
  });
  const shortcuts = useCanvasShortcuts({
    editActions,
    instance: viewport.instance,
    nodes: drawGraph.nodes,
    selectedIds: elements.selectedIds,
    selectedNodes,
    setSelection: elements.setSelection,
    viewportSize: viewport.viewportSize,
  });
  const presence = useFlowPresence({
    workspaceId: props.workspaceId,
    instance: viewport.instance,
    selectedIds: elements.selectedIds,
    view: viewport.view,
    onPresenceChange: props.onPresenceChange,
  });

  useEffect(() => {
    const scope = `${props.workspaceId}\0${props.versionId}`;
    const selectedIds = [...elements.selectedIds];
    const signature = selectedIds.join('\0');
    if (reportedSelectionRef.current.scope !== scope) {
      reportedSelectionRef.current = { scope, signature: '' };
      elements.setSelection([]);
      onSelectionChangeRef.current?.([]);
      return;
    }
    if (reportedSelectionRef.current.signature === signature) return;
    reportedSelectionRef.current.signature = signature;
    onSelectionChangeRef.current?.(selectedIds);
  }, [elements.selectedIds, elements.setSelection, props.versionId, props.workspaceId]);

  return (
    <section ref={viewport.canvasRef} className={typeof window === 'undefined' ? 'p-canvas cv-bold' : 'p-canvas cv-bold flow-canvas'} aria-label="Workflow graph canvas" tabIndex={0} onKeyDown={shortcuts}>
      {typeof window === 'undefined' ? (
        <StaticFlowContent baseEdgeIds={baseEdgeIds} comments={props.comments ?? []} edges={drawGraph.edges} nodes={elements.nodes} presenceByActor={props.presenceByActor} setSelection={elements.setSelection} view={viewport.view} />
      ) : (
        <ReactFlow
        nodes={flowNodes}
        edges={flowEdges}
        nodeTypes={NODE_TYPES}
        colorMode="dark"
        defaultViewport={{ x: viewport.view.x, y: viewport.view.y, zoom: viewport.view.z }}
        deleteKeyCode={null}
        elementsSelectable={capabilities.select}
        fitViewOptions={{ padding: 0.18 }}
        isValidConnection={flowActions.isValidConnection}
        maxZoom={GRAPH_CANVAS_MAX_ZOOM}
        minZoom={GRAPH_CANVAS_MIN_ZOOM}
        nodesConnectable={capabilities.connect}
        nodesDraggable={capabilities.move}
        onlyRenderVisibleElements={typeof ResizeObserver !== 'undefined'}
        panOnDrag={activeMode === 'edit' ? [1, 2] : true}
        selectionMode={SelectionMode.Partial}
        selectionOnDrag={activeMode === 'edit'}
        snapGrid={[20, 20]}
        onConnect={flowActions.connect}
        onEdgeDoubleClick={(_event, edge) => capabilities.connect && flowActions.disconnect(edge)}
        onEdgesChange={elements.onEdgesChange}
        onInit={viewport.onInit}
        onMove={viewport.onMove}
        onMoveEnd={viewport.onMoveEnd}
        onNodeDragStop={(_event, node, nodes) => flowActions.commitMove(nodes.length ? nodes : [node])}
        onNodesChange={elements.onNodesChange}
        onPaneMouseLeave={presence.onPaneMouseLeave}
        onPaneMouseMove={presence.onPaneMouseMove}
        onDrop={(event) => handleFlowDrop(event, viewport.instance, catalogState.definitionByType, editActions.addNode)}
        onDragOver={(event) => handleFlowDragOver(event, capabilities.paste)}
      >
        <Background color="rgba(255,255,255,.22)" gap={20} size={1} variant={BackgroundVariant.Dots} />
        <ViewportPortal>
          {useOverview ? <CanvasOverview nodes={drawGraph.nodes} /> : null}
          {props.presenceByActor ? (
            <CanvasCollaborationWorld comments={props.comments ?? []} hiddenActorId={LOCAL_CANVAS_ACTOR.actorId} nodes={drawGraph.nodes} presenceByActor={props.presenceByActor} />
          ) : (
            <ConnectedCanvasCollaborationWorld comments={props.comments ?? []} nodes={drawGraph.nodes} />
          )}
        </ViewportPortal>
        <CanvasViewControls instance={viewport.instance} nodes={drawGraph.nodes} view={viewport.view} viewportSize={viewport.viewportSize} />
        </ReactFlow>
      )}
      <CanvasOverlays {...props} canEdit={capabilities.move} catalog={catalogState.catalog} catalogError={catalogState.catalogError} definitionByType={catalogState.definitionByType} drawGraph={drawGraph} editActions={editActions} editStatus={editStatus} modelCatalog={catalogState.modelCatalog} modelCatalogError={catalogState.modelCatalogError} pendingProposal={Boolean(props.pendingProposal)} readiness={implementation.readiness} resolution={implementation.resolution} selectedIds={elements.selectedIds} selectedNodes={selectedNodes} setSelection={elements.setSelection} view={viewport.view} viewportSize={viewport.viewportSize} />
    </section>
  );
}
