import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Background,
  BackgroundVariant,
  ConnectionMode,
  ReactFlow,
  SelectionMode,
  ViewportPortal,
  type EdgeTypes,
  type NodeTypes,
} from '@xyflow/react';
import { LOCAL_CANVAS_ACTOR } from '../../canvas-presence';
import {
  CanvasCollaborationWorld,
  ConnectedCanvasCollaborationWorld,
} from '../graph-canvas-collaboration';
import { artifactsForNode, resolveGridSplitSource } from '../../grid-split';
import { outputsByCanvasNode } from '../graph-canvas-artifacts';
import { canvasCapabilities } from '../graph-canvas-capabilities';
import { createCanvasEditActions } from '../graph-canvas-edit-actions';
import {
  GRAPH_CANVAS_MAX_ZOOM,
  GRAPH_CANVAS_MIN_ZOOM,
  computeMinimapLayout,
  viewForMinimapPoint,
} from '../graph-canvas-navigation';
import {
  applyImageProcessingNodeStates,
  buildComparableNodeMap,
  buildEdgeSignatureSet,
  buildRunStepStateMap,
  edgesForViewport,
  nodeIdsForViewport,
} from '../graph-canvas-rendering';
import type { GraphCanvasProps } from '../graph-canvas-types';
import { selectedCanvasNodes, toWorkflowFlowEdges, toWorkflowFlowNodes } from './adapter';
import { CardConnectionLine, CardEdge } from './card-edge';
import { alignNodePositions, type AlignKind } from '../graph-canvas-align';
import {
  applyGroupSelection,
  applyUngroupSelection,
  canGroupSelectedNodes,
  canUngroupSelectedNodes,
  expandSelectionWithGroups,
  groupFrames,
  loadCanvasGroups,
  saveCanvasGroups,
  type CanvasGroup,
} from '../graph-canvas-groups';
import { CanvasMinimap } from '../graph-canvas-minimap';
import { relatedHighlight } from '../graph-canvas-relations';
import { CanvasViewControls } from './controls';
import { ReactFlowWorkflowNode } from './node';
import { CanvasOverlays, type CanvasAddMenu } from './overlays';
import { CanvasOverview, shouldUseCanvasOverview } from './overview';
import { StaticFlowContent } from './static-content';
import { useCanvasCatalog, useImplementationResolution } from './use-catalog';
import { useCanvasPointerTools } from './use-canvas-tools';
import {
  emptyMediaCardIdFromDropTarget,
  handleFlowDragOver,
  handleFlowDrop,
  useFlowActions,
} from './use-flow-actions';
import { useFlowElements } from './use-flow-elements';
import { useFlowPresence } from './use-presence';
import { useCanvasShortcuts } from './use-shortcuts';
import { setFlowViewportToNodes, useFlowViewport } from './use-viewport';
import { useWorkbenchStore } from '../../store';
import type { CanvasSnapshotUpdate } from '../../types';

const NODE_TYPES: NodeTypes = { workflow: ReactFlowWorkflowNode };
const EDGE_TYPES: EdgeTypes = { card: CardEdge };

export function GraphCanvas(props: GraphCanvasProps) {
  const [editStatus, setEditStatus] = useState<string | null>(null);
  const [addMenu, setAddMenu] = useState<CanvasAddMenu | null>(null);
  const [editingTextNodeId, setEditingTextNodeId] = useState<string | null>(null);
  const connectStartRef = useRef<{ x: number; y: number; nodeId: string; handleType: 'source' | 'target' } | null>(null);
  const onSelectionChangeRef = useRef(props.onSelectionChange);
  const onSelectOutputRef = useRef(props.onSelectOutput);
  const reportedSelectionRef = useRef({ scope: '', signature: '' });
  const resetFlowNodesRef = useRef<() => void>(() => undefined);
  onSelectionChangeRef.current = props.onSelectionChange;
  onSelectOutputRef.current = props.onSelectOutput;
  const selectOutput = useCallback((id: string) => onSelectOutputRef.current?.(id), []);
  const ingestMediaFiles = useWorkbenchStore((store) => store.ingestMediaFiles);
  const replaceNodeMedia = useWorkbenchStore((store) => store.replaceNodeMedia);
  const undoVersion = useWorkbenchStore((store) => store.undoVersion);
  const restoreVersion = useWorkbenchStore((store) => store.restoreVersion);
  const redoStackRef = useRef<string[]>([]);
  const layoutUndoRef = useRef<CanvasSnapshotUpdate[]>([]);
  const layoutRedoRef = useRef<CanvasSnapshotUpdate[]>([]);
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);
  const [referencePickerNodeId, setReferencePickerNodeId] = useState<string | null>(null);
  const [minimapOpen, setMinimapOpen] = useState(true);
  const [groups, setGroups] = useState<CanvasGroup[]>(() => loadCanvasGroups(props.workspaceId));
  const resetRejectedMutation = useCallback(() => resetFlowNodesRef.current(), []);
  const sourceGraph = props.canvasGraph ?? props.graph;
  const drawGraph = props.pendingProposal?.previewGraph ?? sourceGraph;
  const activeMode = props.pendingProposal ? 'review' : props.onCreateProposal ? 'edit' : 'view';
  const capabilities = canvasCapabilities(activeMode, Boolean(props.onCreateProposal));
  const pointerTools = useCanvasPointerTools(activeMode === 'edit');
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
  const openAddMenu = useCallback((
    clientX: number,
    clientY: number,
    connectFrom?: CanvasAddMenu['connectFrom'],
  ) => {
    const flow = viewport.instance?.screenToFlowPosition({ x: clientX, y: clientY });
    if (!flow) return;
    setAddMenu({ clientX, clientY, flow, connectFrom });
  }, [viewport.instance]);
  const catalogState = useCanvasCatalog();
  const baseComparableById = useMemo(
    () => buildComparableNodeMap(sourceGraph.nodes),
    [sourceGraph.nodes],
  );
  const baseEdgeIds = useMemo(() => buildEdgeSignatureSet(sourceGraph.edges), [sourceGraph.edges]);
  const outputsByNodeId = useMemo(() => outputsByCanvasNode(props.outputs), [props.outputs]);
  const stepSignature = props.run.steps.map((step) => `${step.nodeId}:${step.state}`).join('|');
  const imageJobSignature = (props.imageProcessingJobs ?? [])
    .map((job) => `${job.id}:${job.status}:${job.sourceNodeId}:${job.resultNodeId ?? ''}`)
    .join('|');
  const stepStateByNodeId = useMemo(
    () => applyImageProcessingNodeStates(
      buildRunStepStateMap(props.run.steps),
      props.imageProcessingJobs ?? [],
    ),
    [imageJobSignature, stepSignature],
  );
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
    workspaceId: props.workspaceId,
    onUploadMedia: capabilities.paste
      ? (nodeId, file) => { void replaceNodeMedia(nodeId, file); }
      : undefined,
    onHandleClick: capabilities.paste
      ? (nodeId, handleType, clientX, clientY) => {
          openAddMenu(clientX, clientY, { nodeId, handleType });
        }
      : undefined,
  }), [
    baseComparableById, capabilities.move, capabilities.paste, capabilities.resize,
    catalogState.definitionByType, drawGraph.nodes, flowActions.commitResize, openAddMenu,
    outputsByNodeId, replaceNodeMedia, selectOutput, props.pendingProposal, props.workflowGraph,
    props.workspaceId, stepStateByNodeId,
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
  const adaptedEdges = useMemo(
    () => toWorkflowFlowEdges(renderEdges, baseEdgeIds, Boolean(props.pendingProposal)),
    [baseEdgeIds, props.pendingProposal, renderEdges],
  );
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
  const related = useMemo(
    () => relatedHighlight(
      hoveredNodeId ?? (elements.selectedIds.size === 1 ? [...elements.selectedIds][0] : null),
      drawGraph.edges,
    ),
    [drawGraph.edges, elements.selectedIds, hoveredNodeId],
  );
  const flowNodes = useMemo(
    () => {
      const visible = useOverview
        ? []
        : renderNodeIds
        ? elements.nodes.filter((node) => renderNodeIds.has(node.id))
        : elements.nodes;
      return visible.map((node) => {
        const classes = [
          node.className,
          related.nodeIds.has(node.id) ? 'is-related' : '',
          referencePickerNodeId === node.id ? 'is-ref-target' : '',
          referencePickerNodeId && referencePickerNodeId !== node.id ? 'is-ref-available' : '',
        ].filter(Boolean);
        return classes.length > 0 ? { ...node, className: classes.join(' ') } : node;
      });
    },
    [elements.nodes, referencePickerNodeId, related.nodeIds, renderNodeIds, useOverview],
  );
  const flowEdges = useMemo(
    () => {
      const visible = useOverview
        ? []
        : renderNodeIds
        ? elements.edges.filter(
          (edge) => renderNodeIds.has(edge.source) && renderNodeIds.has(edge.target),
        )
        : elements.edges;
      return visible.map((edge) => (
        related.edgeIds.has(edge.id) || edge.selected
          ? {
              ...edge,
              className: edge.selected ? 'is-selected' : 'is-related',
              style: {
                ...edge.style,
                strokeWidth: edge.selected ? 3 : 2.4,
              },
            }
          : edge
      ));
    },
    [elements.edges, related.edgeIds, renderNodeIds, useOverview],
  );
  const liveGraphNodes = useMemo(
    () => selectedCanvasNodes(
      drawGraph.nodes,
      new Set(drawGraph.nodes.map((node) => node.id)),
      elements.nodes,
    ),
    [drawGraph.nodes, elements.nodes],
  );
  const frames = useMemo(() => groupFrames(groups, liveGraphNodes), [groups, liveGraphNodes]);
  const chromeHidden = viewport.moving;
  const overlayView = viewport.liveView;
  const selectedNodes = useMemo(
    () => selectedCanvasNodes(drawGraph.nodes, elements.selectedIds, elements.nodes),
    [drawGraph.nodes, elements.nodes, elements.selectedIds],
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
  const pasteMedia = useCallback(async () => {
    try {
      if (capabilities.paste && navigator.clipboard?.read) {
        const files: File[] = [];
        for (const item of await navigator.clipboard.read()) {
          const type = item.types.find(
            (value) => value.startsWith('image/') || value.startsWith('video/') || value.startsWith('audio/'),
          );
          if (!type) continue;
          const blob = await item.getType(type);
          files.push(new File([blob], clipboardFilename(type), { type }));
        }
        if (files.length > 0) {
          await ingestMediaFiles(files, viewport.instance?.screenToFlowPosition({
            x: viewport.viewportSize.width / 2,
            y: viewport.viewportSize.height / 2,
          }));
          return;
        }
      }
    } catch {
      // Browser may deny clipboard.read(); subgraph paste still applies.
    }
    await editActions.pasteSelection();
  }, [capabilities.paste, editActions, ingestMediaFiles, viewport.instance, viewport.viewportSize]);
  const persistGroups = useCallback((next: CanvasGroup[]) => {
    setGroups(next);
    saveCanvasGroups(props.workspaceId, next);
  }, [props.workspaceId]);
  const pushLayoutUndo = useCallback((entry: CanvasSnapshotUpdate) => {
    layoutUndoRef.current.push(entry);
    layoutRedoRef.current = [];
  }, []);
  const undoCanvas = useCallback(() => {
    if (!capabilities.paste) return;
    const layout = layoutUndoRef.current.pop();
    if (layout) {
      const current: CanvasSnapshotUpdate = {
        positions: layout.positions?.map((item) => {
          const node = drawGraph.nodes.find((entry) => entry.id === item.id);
          return node ? { id: item.id, x: node.position.x, y: node.position.y } : item;
        }),
        sizes: layout.sizes?.map((item) => {
          const node = drawGraph.nodes.find((entry) => entry.id === item.id);
          return node?.size ? { id: item.id, width: node.size.width, height: node.size.height } : item;
        }),
      };
      layoutRedoRef.current.push(current);
      void props.onSaveCanvasSnapshot?.(layout);
      return;
    }
    redoStackRef.current.push(props.versionId);
    void undoVersion().catch((error) => {
      setEditStatus(error instanceof Error ? error.message : '撤销失败');
    });
  }, [capabilities.paste, drawGraph.nodes, props.onSaveCanvasSnapshot, props.versionId, undoVersion]);
  const redoCanvas = useCallback(() => {
    if (!capabilities.paste) return;
    const layout = layoutRedoRef.current.pop();
    if (layout) {
      layoutUndoRef.current.push({
        positions: layout.positions?.map((item) => {
          const node = drawGraph.nodes.find((entry) => entry.id === item.id);
          return node ? { id: item.id, x: node.position.x, y: node.position.y } : item;
        }),
      });
      void props.onSaveCanvasSnapshot?.(layout);
      return;
    }
    const versionId = redoStackRef.current.pop();
    if (!versionId) return;
    void restoreVersion(versionId).catch((error) => {
      setEditStatus(error instanceof Error ? error.message : '重做失败');
    });
  }, [capabilities.paste, drawGraph.nodes, props.onSaveCanvasSnapshot, restoreVersion]);
  const duplicateSelection = useCallback(() => {
    selectedNodes.forEach((node) => editActions.duplicateNode(node));
  }, [editActions, selectedNodes]);
  const groupSelection = useCallback(() => {
    if (!canGroupSelectedNodes(elements.selectedIds)) return;
    persistGroups(applyGroupSelection(elements.selectedIds, groups));
  }, [elements.selectedIds, groups, persistGroups]);
  const ungroupSelection = useCallback(() => {
    if (!canUngroupSelectedNodes(elements.selectedIds, groups)) return;
    persistGroups(applyUngroupSelection(elements.selectedIds, groups));
  }, [elements.selectedIds, groups, persistGroups]);
  const deleteSelectedEdges = useCallback(() => {
    for (const edge of elements.edges) {
      if (edge.selected) flowActions.disconnect(edge);
    }
  }, [elements.edges, flowActions]);
  const alignSelection = useCallback((kind: AlignKind) => {
    const positions = alignNodePositions(selectedNodes, kind);
    if (positions.length === 0) return;
    pushLayoutUndo({
      positions: positions.map((item) => {
        const node = drawGraph.nodes.find((entry) => entry.id === item.id);
        return { id: item.id, x: node?.position.x ?? item.x, y: node?.position.y ?? item.y };
      }),
    });
    void props.onSaveCanvasSnapshot?.({ positions });
  }, [drawGraph.nodes, props.onSaveCanvasSnapshot, pushLayoutUndo, selectedNodes]);
  const shortcuts = useCanvasShortcuts({
    editActions,
    hasSelectedEdges: elements.selectedEdgeIds.size > 0,
    instance: viewport.instance,
    nodes: drawGraph.nodes,
    onDeleteEdges: deleteSelectedEdges,
    onDuplicate: duplicateSelection,
    onGroup: groupSelection,
    onRedo: redoCanvas,
    onUndo: undoCanvas,
    onUngroup: ungroupSelection,
    pasteMedia,
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
    const scope = props.workspaceId;
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
  }, [elements.selectedIds, elements.setSelection, props.workspaceId]);

  useEffect(() => {
    setGroups(loadCanvasGroups(props.workspaceId));
  }, [props.workspaceId]);
  useEffect(() => {
    if (!referencePickerNodeId) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setReferencePickerNodeId(null);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [referencePickerNodeId]);
  useEffect(() => {
    if (editingTextNodeId && !elements.selectedIds.has(editingTextNodeId)) {
      setEditingTextNodeId(null);
    }
  }, [editingTextNodeId, elements.selectedIds]);
  useEffect(() => {
    const expanded = expandSelectionWithGroups(elements.selectedIds, groups);
    if (expanded.size === elements.selectedIds.size
      && [...expanded].every((id) => elements.selectedIds.has(id))) {
      return;
    }
    elements.setSelection(expanded);
  }, [elements.selectedIds, elements.setSelection, groups]);

  return (
    <section
      ref={viewport.canvasRef}
      className={typeof window === 'undefined'
        ? 'p-canvas cv-bold'
        : `p-canvas cv-bold flow-canvas${pointerTools.activeTool === 'pan' ? ' flow-canvas--pan' : ''}`}
      aria-label="Workflow graph canvas"
      tabIndex={0}
      onKeyDown={shortcuts}
      onDoubleClick={(event) => {
        if (!capabilities.paste) return;
        const target = event.target;
        if (!(target instanceof Element)) return;
        if (!target.closest('.react-flow__pane')) return;
        if (target.closest('.react-flow__node, .react-flow__handle, .canvas-add-menu, .node-library-bar')) return;
        openAddMenu(event.clientX, event.clientY);
      }}
    >
      {typeof window === 'undefined' ? (
        <StaticFlowContent baseEdgeIds={baseEdgeIds} comments={props.comments ?? []} edges={drawGraph.edges} nodes={elements.nodes} presenceByActor={props.presenceByActor} setSelection={elements.setSelection} view={viewport.view} />
      ) : (
        <ReactFlow
        nodes={flowNodes}
        edges={flowEdges}
        nodeTypes={NODE_TYPES}
        edgeTypes={EDGE_TYPES}
        connectionLineComponent={CardConnectionLine}
        colorMode="dark"
        defaultViewport={{ x: viewport.view.x, y: viewport.view.y, zoom: viewport.view.z }}
        deleteKeyCode={null}
        elementsSelectable={capabilities.select}
        fitViewOptions={{ padding: 0.18 }}
        connectionLineStyle={{ stroke: '#888', strokeWidth: 2, strokeDasharray: '6 6' }}
        connectionMode={ConnectionMode.Loose}
        connectionRadius={120}
        isValidConnection={flowActions.isValidConnection}
        maxZoom={GRAPH_CANVAS_MAX_ZOOM}
        minZoom={GRAPH_CANVAS_MIN_ZOOM}
        nodesConnectable={capabilities.connect}
        nodesDraggable={capabilities.move}
        onlyRenderVisibleElements={typeof ResizeObserver !== 'undefined'}
        panOnDrag={pointerTools.panOnDrag}
        zoomOnDoubleClick={false}
        connectOnClick={false}
        selectionMode={SelectionMode.Partial}
        selectionOnDrag={pointerTools.selectionOnDrag}
        snapGrid={[20, 20]}
        onConnect={flowActions.connect}
        onConnectStart={(event, params) => {
          document.body.classList.add('connecting');
          const point = eventClientPoint(event);
          connectStartRef.current = {
            x: point.x,
            y: point.y,
            nodeId: params.nodeId ?? '',
            handleType: params.handleType === 'target' ? 'target' : 'source',
          };
        }}
        onConnectEnd={(event, state) => {
          document.body.classList.remove('connecting');
          const start = connectStartRef.current;
          connectStartRef.current = null;
          if (!capabilities.connect) return;
          const fromId = state.fromNode?.id ?? start?.nodeId;
          const point = eventClientPoint(event);
          const hit = document.elementFromPoint(point.x, point.y)?.closest('.react-flow__node');
          const toId = state.toNode?.id ?? hit?.getAttribute('data-id');
          if (fromId && toId && toId !== fromId) {
            if ((state.fromHandle?.type ?? start?.handleType) === 'target') {
              flowActions.connectNodes(toId, fromId);
            } else {
              flowActions.connectNodes(fromId, toId);
            }
            return;
          }
          if (capabilities.paste && start?.nodeId) {
            openAddMenu(point.x, point.y, { nodeId: start.nodeId, handleType: start.handleType });
          }
        }}
        onPaneClick={(event) => {
          if (referencePickerNodeId) {
            setReferencePickerNodeId(null);
            return;
          }
          if (event.detail === 2 && capabilities.paste) {
            openAddMenu(event.clientX, event.clientY);
            return;
          }
          if (event.detail === 1) setAddMenu(null);
        }}
        onNodeClick={(_event, node) => {
          if (!referencePickerNodeId || node.id === referencePickerNodeId) return;
          flowActions.connectNodes(node.id, referencePickerNodeId);
          setReferencePickerNodeId(null);
          setEditStatus('已接上参考图');
        }}
        onPaneContextMenu={(event) => {
          event.preventDefault();
          if (!capabilities.paste) return;
          openAddMenu(event.clientX, event.clientY);
        }}
        onEdgeClick={(_event, edge) => elements.setEdgeSelection([edge.id])}
        onEdgeDoubleClick={(_event, edge) => capabilities.connect && flowActions.disconnect(edge)}
        onEdgesChange={elements.onEdgesChange}
        onInit={viewport.onInit}
        onMove={viewport.onMove}
        onMoveEnd={viewport.onMoveEnd}
        onNodeDoubleClick={(_event, node) => {
          if (!capabilities.move) return;
          if (node.data.node.nodeType === 'input.text') setEditingTextNodeId(node.id);
        }}
        onNodeDragStop={(_event, node, nodes) => {
          const moved = nodes.length ? nodes : [node];
          const previous = moved.flatMap((item) => {
            const base = drawGraph.nodes.find((entry) => entry.id === item.id);
            return base ? [{ id: item.id, x: base.position.x, y: base.position.y }] : [];
          });
          if (previous.length > 0) pushLayoutUndo({ positions: previous });
          flowActions.commitMove(moved);
        }}
        onNodeMouseEnter={(_event, node) => setHoveredNodeId(node.id)}
        onNodeMouseLeave={() => setHoveredNodeId(null)}
        onNodesChange={elements.onNodesChange}
        onPaneMouseLeave={presence.onPaneMouseLeave}
        onPaneMouseMove={presence.onPaneMouseMove}
        onDrop={(event) => {
          const files = [...event.dataTransfer.files];
          if (files.length > 0 && capabilities.paste) {
            event.preventDefault();
            const emptyId = emptyMediaCardIdFromDropTarget(
              event.target,
              drawGraph.nodes,
              (node) => !resolveGridSplitSource({
                nodeType: node.nodeType,
                params: props.workflowGraph?.nodes[node.id]?.params,
                artifacts: artifactsForNode(props.outputs, node.id),
              }),
            );
            if (emptyId && files[0]) {
              void replaceNodeMedia(emptyId, files[0]);
              return;
            }
            const position = viewport.instance?.screenToFlowPosition({
              x: event.clientX,
              y: event.clientY,
            });
            void ingestMediaFiles(files, position);
            return;
          }
          handleFlowDrop(event, viewport.instance, catalogState.definitionByType, editActions.addNode);
        }}
        onDragOver={(event) => {
          if (capabilities.paste && [...event.dataTransfer.types].includes('Files')) {
            event.preventDefault();
            event.dataTransfer.dropEffect = 'copy';
            return;
          }
          handleFlowDragOver(event, capabilities.paste);
        }}
      >
        <Background color="rgba(255,255,255,.22)" gap={20} size={1} variant={BackgroundVariant.Dots} />
        <ViewportPortal>
          {useOverview ? <CanvasOverview nodes={drawGraph.nodes} /> : null}
          {frames.map((frame) => (
            <div className="canvas-group-frame" key={frame.id} style={frame}>
              <span>组</span>
            </div>
          ))}
          {props.presenceByActor ? (
            <CanvasCollaborationWorld comments={props.comments ?? []} hiddenActorId={LOCAL_CANVAS_ACTOR.actorId} nodes={drawGraph.nodes} presenceByActor={props.presenceByActor} />
          ) : (
            <ConnectedCanvasCollaborationWorld comments={props.comments ?? []} nodes={drawGraph.nodes} />
          )}
        </ViewportPortal>
        <CanvasViewControls
          activeTool={pointerTools.activeTool}
          canEdit={activeMode === 'edit'}
          instance={viewport.instance}
          minimapOpen={minimapOpen}
          nodes={drawGraph.nodes}
          onToolChange={pointerTools.setTool}
          onToggleMinimap={() => setMinimapOpen((open) => !open)}
          view={overlayView}
          viewportSize={viewport.viewportSize}
        />
        </ReactFlow>
      )}
      {minimapOpen && !useOverview ? (
        <CanvasMinimapHost
          nodes={drawGraph.nodes}
          view={overlayView}
          viewportSize={viewport.viewportSize}
          onNavigate={(view) => {
            void viewport.instance?.setViewport({ x: view.x, y: view.y, zoom: view.z }, { duration: 180 });
          }}
        />
      ) : null}
      <CanvasOverlays {...props} addMenu={addMenu} canEdit={capabilities.move} catalog={catalogState.catalog} catalogError={catalogState.catalogError} chromeHidden={chromeHidden} definitionByType={catalogState.definitionByType} drawGraph={drawGraph} editActions={editActions} editingTextNodeId={editingTextNodeId} editStatus={editStatus} modelCatalog={catalogState.modelCatalog} modelCatalogError={catalogState.modelCatalogError} onAlign={alignSelection} onConnectReference={(sourceId, targetId) => flowActions.connectNodes(sourceId, targetId)} onGroup={groupSelection} onUngroup={ungroupSelection} onJumpNode={(nodeId) => { const node = drawGraph.nodes.find((item) => item.id === nodeId); if (node) void setFlowViewportToNodes(viewport.instance, [node], viewport.viewportSize); }} onRedo={redoCanvas} onStartReferencePicker={(nodeId) => setReferencePickerNodeId((current) => current === nodeId ? null : nodeId)} onUndo={undoCanvas} pendingProposal={Boolean(props.pendingProposal)} readiness={implementation.readiness} referencePickerNodeId={referencePickerNodeId} resolution={implementation.resolution} selectedIds={elements.selectedIds} selectedNodes={selectedNodes} setAddMenu={setAddMenu} setEditStatus={setEditStatus} setSelection={elements.setSelection} view={overlayView} viewportSize={viewport.viewportSize} />
    </section>
  );
}

function CanvasMinimapHost({
  nodes,
  onNavigate,
  view,
  viewportSize,
}: {
  nodes: GraphCanvasProps['graph']['nodes'];
  onNavigate: (view: { x: number; y: number; z: number }) => void;
  view: { x: number; y: number; z: number };
  viewportSize: { width: number; height: number };
}) {
  const layout = computeMinimapLayout(nodes);
  if (!layout) return null;
  return (
    <CanvasMinimap
      layout={layout}
      view={view}
      viewportSize={viewportSize}
      onNavigate={(x, y) => onNavigate(viewForMinimapPoint(layout, { x, y }, view, viewportSize))}
    />
  );
}

function eventClientPoint(event: MouseEvent | TouchEvent): { x: number; y: number } {
  if ('changedTouches' in event) {
    const touch = event.changedTouches[0];
    return { x: touch?.clientX ?? 0, y: touch?.clientY ?? 0 };
  }
  return { x: event.clientX, y: event.clientY };
}

function clipboardFilename(type: string): string {
  if (type.startsWith('video/')) return 'clipboard.mp4';
  if (type.startsWith('audio/')) return 'clipboard.mp3';
  return 'clipboard.png';
}
