import type { StoreApi } from 'zustand';
import { uploadWorkspaceImage } from './api';
import { buildGridSplitProposalInput } from './components/graph-canvas-editing';
import imageProcessors from './cuter-image-processors';
import {
  artifactsForNode,
  fetchArtifactBlob,
  fetchWorkspaceUploadContent,
  fitMediaNodeSize,
  gridSplitFilename,
  gridSplitTilePositions,
  resolveGridSplitSource,
  validateGridSplitAxes,
} from './grid-split';
import { appendSystemError } from './store-model';
import type { WorkbenchStore } from './store-types';
import { WorkspaceChangedError } from './workspace-action-guard';
import type { WorkspaceRequestScope } from './workspace-request-scope';

export function createGridSplitAction(
  set: StoreApi<WorkbenchStore>['setState'],
  get: StoreApi<WorkbenchStore>['getState'],
  requestScope: WorkspaceRequestScope,
): (nodeId: string, rows: number, columns: number) => Promise<void> {
  return async (nodeId, rows, columns) => {
    validateGridSplitAxes(rows, columns);
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
    if (!source) throw new Error('当前节点没有可切分的图片');

    try {
      const blob = await readSourceBlob(workspaceId, source, requestScope.signal());
      if (!requestScope.isActive(generation, workspaceId)) {
        throw new WorkspaceChangedError('workspace changed while reading the source image');
      }
      if (blob.type && !blob.type.startsWith('image/')) {
        throw new Error('宫格切分只支持图片');
      }
      const outputs = await imageProcessors.splitImage(blob, { rows, columns });
      if (outputs.length === 0) {
        throw new Error('宫格切分没有产出切片');
      }
      const uploads = [];
      for (const [index, output] of outputs.entries()) {
        const file = new File(
          [output.blob],
          gridSplitFilename(sourceNode.title || nodeId, output.tile.row, output.tile.column),
          { type: 'image/png' },
        );
        try {
          uploads.push(await uploadWorkspaceImage(workspaceId, file, requestScope.signal()));
        } catch (error) {
          const message = error instanceof Error ? error.message : 'image upload failed';
          throw new Error(`宫格切分失败：已上传 ${index} 张切片，未写入画布。${message}`);
        }
        if (!requestScope.isActive(generation, workspaceId)) {
          throw new WorkspaceChangedError('workspace changed while uploading grid tiles');
        }
      }
      const sizes = outputs.map((output) => fitMediaNodeSize(output.width, output.height));
      const tileWidth = Math.max(...sizes.map((size) => size.width));
      const tileHeight = Math.max(...sizes.map((size) => size.height));
      await get().appendManualEdit(
        buildGridSplitProposalInput({
          baseVersionId: state.workspace.versionId,
          existingNodeIds: state.graph.nodes.map((node) => node.id),
          placements: gridSplitTilePositions({
            source: sourceNode,
            rows,
            columns,
            tileWidth,
            tileHeight,
          }),
          sourceNodeId: nodeId,
          sourceTitle: sourceNode.title || nodeId,
          tiles: uploads.map((upload, index) => ({
            storageUri: upload.storageUri,
            row: outputs[index]!.tile.row,
            column: outputs[index]!.tile.column,
            size: sizes[index],
          })),
        }),
      );
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
      const message = error instanceof Error ? error.message : '宫格切分失败';
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
