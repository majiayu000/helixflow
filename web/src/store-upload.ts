import type { StoreApi } from 'zustand';
import { uploadWorkspaceImage } from './api';
import {
  buildCropResultProposalInput,
  buildMediaIngestProposalInput,
} from './components/graph-canvas-editing';
import { graphNodeWidth } from './components/graph-canvas-navigation';
import {
  artifactsForNode,
  cropImageBlob,
  fetchArtifactBlob,
  fetchWorkspaceUploadContent,
  fitMediaNodeSize,
  nodeTypeFromMime,
  readNaturalImageSize,
  resolveGridSplitSource,
  type PixelCropRect,
} from './grid-split';
import { createGridSplitAction } from './store-grid-split';
import {
  createGenerateFromMediaCardAction,
  createImageCanvasToolAction,
} from './store-image-edit';
import { appendSystemError } from './store-model';
import type { WorkbenchStore } from './store-types';
import { buildSetParamEditInput } from './workbench-edit-session';
import type { WorkspaceRequestScope } from './workspace-request-scope';
import { WorkspaceChangedError } from './workspace-action-guard';

export { createGridSplitAction } from './store-grid-split';
export { createImageCanvasToolAction } from './store-image-edit';

export function createImageMediaActions(
  set: StoreApi<WorkbenchStore>['setState'],
  get: StoreApi<WorkbenchStore>['getState'],
  requestScope: WorkspaceRequestScope,
) {
  return {
    uploadImage: createIngestMediaAction(set, get, requestScope),
    ingestMediaFiles: createIngestMediaAction(set, get, requestScope),
    splitImageGrid: createGridSplitAction(set, get, requestScope),
    applyImageCanvasTool: createImageCanvasToolAction(set, get, requestScope),
    generateFromMediaCard: createGenerateFromMediaCardAction(set, get, requestScope),
    cropImageNode: createCropImageAction(set, get, requestScope),
    replaceNodeMedia: createReplaceNodeMediaAction(set, get, requestScope),
  };
}

function pendingNodeIds(get: StoreApi<WorkbenchStore>['getState']): string[] {
  return (
    get().editSession?.ops
      .filter((op) => op.op === 'spawn_node')
      .map((op) => op.id) ?? []
  );
}

function createIngestMediaAction(
  set: StoreApi<WorkbenchStore>['setState'],
  get: StoreApi<WorkbenchStore>['getState'],
  requestScope: WorkspaceRequestScope,
): (files: File | File[], position?: { x: number; y: number }) => Promise<void> {
  return async (files, position) => {
    const list = (Array.isArray(files) ? files : [files]).filter((file) => file.size > 0);
    if (list.length === 0) return;
    const state = get().state;
    if (!state) throw new Error('workspace is not loaded');
    const workspaceId = state.workspace.id;
    const generation = requestScope.currentGeneration();
    try {
      const items = [];
      for (const [index, file] of list.entries()) {
        const uploaded = await uploadWorkspaceImage(workspaceId, file, requestScope.signal());
        if (!requestScope.isActive(generation, workspaceId)) {
          throw new WorkspaceChangedError('workspace changed while uploading media');
        }
        const natural = await readNaturalImageSize(file);
        items.push({
          nodeType: nodeTypeFromMime(uploaded.mime || file.type),
          title: uploaded.filename || file.name || '未命名素材',
          storageUri: uploaded.storageUri,
          position: {
            x: (position?.x ?? 80) + index * 40,
            y: (position?.y ?? 80) + index * 40,
          },
          size: natural ? fitMediaNodeSize(natural.width, natural.height) : undefined,
        });
      }
      await get().appendManualEdit(
        buildMediaIngestProposalInput({
          baseVersionId: state.workspace.versionId,
          existingNodeIds: [
            ...state.graph.nodes.map((node) => node.id),
            ...pendingNodeIds(get),
          ],
          items,
        }),
      );
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
      const message = error instanceof Error ? error.message : '导入素材失败';
      set((current) => ({
        state: current.state?.workspace.id === workspaceId
          ? appendSystemError(current.state, message)
          : current.state,
      }));
      throw error instanceof Error ? error : new Error(message);
    }
  };
}

function createCropImageAction(
  set: StoreApi<WorkbenchStore>['setState'],
  get: StoreApi<WorkbenchStore>['getState'],
  requestScope: WorkspaceRequestScope,
): (nodeId: string, crop: PixelCropRect) => Promise<void> {
  return async (nodeId, crop) => {
    const state = get().state;
    if (!state) throw new Error('workspace is not loaded');
    const workspaceId = state.workspace.id;
    const generation = requestScope.currentGeneration();
    const sourceNode = state.graph.nodes.find((node) => node.id === nodeId);
    const workflowNode = state.workflowGraph?.nodes[nodeId];
    if (!sourceNode) throw new Error('选中节点不存在');
    const source = resolveGridSplitSource({
      nodeType: sourceNode.nodeType,
      params: workflowNode?.params,
      artifacts: artifactsForNode(state.outputs, nodeId),
    });
    if (!source) throw new Error('当前节点没有可裁剪的图片');
    try {
      const blob = await readSourceBlob(workspaceId, source, requestScope.signal());
      if (!requestScope.isActive(generation, workspaceId)) {
        throw new WorkspaceChangedError('workspace changed while reading the source image');
      }
      const cropped = await cropImageBlob(blob, crop);
      const uploaded = await uploadWorkspaceImage(
        workspaceId,
        new File([cropped.blob], `${sourceNode.title || nodeId}-crop.png`, { type: 'image/png' }),
        requestScope.signal(),
      );
      if (!requestScope.isActive(generation, workspaceId)) {
        throw new WorkspaceChangedError('workspace changed while uploading the crop');
      }
      await get().appendManualEdit(
        buildCropResultProposalInput({
          baseVersionId: state.workspace.versionId,
          existingNodeIds: [
            ...state.graph.nodes.map((node) => node.id),
            ...pendingNodeIds(get),
          ],
          sourceNodeId: nodeId,
          sourceTitle: sourceNode.title || nodeId,
          sourceX: sourceNode.position.x,
          sourceY: sourceNode.position.y,
          sourceWidth: graphNodeWidth(sourceNode),
          storageUri: uploaded.storageUri,
          size: fitMediaNodeSize(cropped.width, cropped.height),
        }),
      );
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
      const message = error instanceof Error ? error.message : '裁剪失败';
      set((current) => ({
        state: current.state?.workspace.id === workspaceId
          ? appendSystemError(current.state, message)
          : current.state,
      }));
      throw error instanceof Error ? error : new Error(message);
    }
  };
}

function createReplaceNodeMediaAction(
  set: StoreApi<WorkbenchStore>['setState'],
  get: StoreApi<WorkbenchStore>['getState'],
  requestScope: WorkspaceRequestScope,
): (nodeId: string, file: File) => Promise<void> {
  return async (nodeId, file) => {
    const state = get().state;
    if (!state) throw new Error('workspace is not loaded');
    const workspaceId = state.workspace.id;
    const generation = requestScope.currentGeneration();
    const sourceNode = state.graph.nodes.find((node) => node.id === nodeId);
    if (!sourceNode) throw new Error('选中节点不存在');
    try {
      const uploaded = await uploadWorkspaceImage(workspaceId, file, requestScope.signal());
      if (!requestScope.isActive(generation, workspaceId)) {
        throw new WorkspaceChangedError('workspace changed while replacing media');
      }
      const ops = buildSetParamEditInput(
        state.workspace.versionId,
        state.workflowGraph,
        nodeId,
        'storage_uri',
        uploaded.storageUri,
      );
      const natural = await readNaturalImageSize(file);
      if (natural) {
        const size = fitMediaNodeSize(natural.width, natural.height);
        ops.ops.push({
          op: 'resize_node',
          id: nodeId,
          size: [size.width, size.height],
        });
        ops.label = `替换 ${sourceNode.title || nodeId}`;
      }
      await get().appendManualEdit(ops);
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
      const message = error instanceof Error ? error.message : '替换失败';
      set((current) => ({
        state: current.state?.workspace.id === workspaceId
          ? appendSystemError(current.state, message)
          : current.state,
      }));
      throw error instanceof Error ? error : new Error(message);
    }
  };
}

async function readSourceBlob(
  workspaceId: string,
  source: { kind: 'upload'; uploadId: string } | { kind: 'artifact'; artifactId: string },
  signal?: AbortSignal,
): Promise<Blob> {
  if (source.kind === 'upload') {
    return fetchWorkspaceUploadContent(workspaceId, source.uploadId, signal);
  }
  return fetchArtifactBlob(source.artifactId, signal);
}
