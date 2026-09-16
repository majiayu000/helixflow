import type { StoreApi } from 'zustand';
import { resolveImplementation, uploadWorkspaceImage } from './api';
import {
  createImageProcessingJob,
  linkImageProcessingResult,
  waitForImageProcessingJob,
} from './api-image-processing';
import { useModelPreferenceStore } from './model-picker';
import {
  buildImageCanvasToolResultProposalInput,
  buildMediaGenerateProposalInput,
} from './components/graph-canvas-editing';
import { graphNodeWidth } from './components/graph-canvas-navigation';
import {
  artifactsForNode,
  fetchArtifactBlob,
  fetchWorkspaceUploadContent,
  fitMediaNodeSize,
  readNaturalImageSize,
  resolveGridSplitSource,
} from './grid-split';
import {
  imageCanvasPrepFilename,
  imageCanvasToolLabel,
  type ImageCanvasToolRequest,
} from './image-canvas-tools';
import { appendSystemError } from './store-model';
import type { WorkbenchStore } from './store-types';
import type { ImageProcessingJob, WorkbenchState } from './types';
import { WorkspaceChangedError } from './workspace-action-guard';
import type { WorkspaceRequestScope } from './workspace-request-scope';

export function createImageCanvasToolAction(
  set: StoreApi<WorkbenchStore>['setState'],
  get: StoreApi<WorkbenchStore>['getState'],
  requestScope: WorkspaceRequestScope,
): (nodeId: string, request: ImageCanvasToolRequest) => Promise<void> {
  return async (nodeId, request) => {
    if ((request.kind === 'outpaint' || request.kind === 'inpaint') && !request.prompt.trim()) {
      throw new Error('请填写修图提示词');
    }
    if ((request.kind === 'outpaint' || request.kind === 'inpaint') && !request.profile.trim()) {
      throw new Error('请选择可用的静图模型');
    }
    const state = get().state;
    if (!state) throw new Error('workspace is not loaded');
    const workspaceId = state.workspace.id;
    const generation = requestScope.currentGeneration();
    const signal = requestScope.signal();
    const sourceNode = state.graph.nodes.find((node) => node.id === nodeId);
    const workflowNode = state.workflowGraph?.nodes[nodeId];
    if (!sourceNode) throw new Error('选中节点不存在');
    const source = resolveGridSplitSource({
      nodeType: sourceNode.nodeType,
      params: workflowNode?.params,
      artifacts: artifactsForNode(state.outputs, nodeId),
    });
    if (!source) {
      throw new Error('当前节点没有可修图的图片');
    }
    const label = imageCanvasToolLabel(request.kind);
    let durableJob: ImageProcessingJob | null = null;

    try {
      const sourceBlob = await readSourceBlob(workspaceId, source, signal);
      if (!requestScope.isActive(generation, workspaceId)) {
        throw new WorkspaceChangedError('workspace changed while reading the source image');
      }
      if (sourceBlob.type && !sourceBlob.type.startsWith('image/')) {
        throw new Error(`${label}只支持图片`);
      }
      const execution = await createImageProcessingJob(
        workspaceId,
        {
          sourceNodeId: nodeId,
          intent: request.kind,
          ...processorRequest(request, sourceBlob),
          filename: imageCanvasPrepFilename(sourceNode.title || nodeId, request.kind),
        },
        signal,
      );
      durableJob = execution.job;
      set((current) => ({
        state: current.state?.workspace.id === workspaceId
          ? withImageProcessingJob(current.state, durableJob as ImageProcessingJob)
          : current.state,
      }));
      durableJob = await waitForImageProcessingJob(workspaceId, durableJob.id, signal);
      set((current) => ({
        state: current.state?.workspace.id === workspaceId
          ? withImageProcessingJob(current.state, durableJob as ImageProcessingJob)
          : current.state,
      }));
      if (durableJob.status !== 'succeeded') {
        throw new Error(durableJob.error || `${label}失败`);
      }
      const outputUploadId = durableJob.outputUploadId;
      if (!outputUploadId) throw new Error('静图任务成功但没有输出文件');
      if (!requestScope.isActive(generation, workspaceId)) {
        throw new WorkspaceChangedError('workspace changed while processing the image');
      }
      const outputBlob = await fetchWorkspaceUploadContent(workspaceId, outputUploadId, signal);
      const outputSize = await readNaturalImageSize(outputBlob);
      if (!outputSize) throw new Error('无法读取静图输出尺寸');
      const uploaded = {
        uploadId: outputUploadId,
        storageUri: `upload://${outputUploadId}`,
        ...outputSize,
      };
      const proposal = buildImageCanvasToolResultProposalInput({
          baseVersionId: state.workspace.versionId,
          existingNodeIds: [
            ...state.graph.nodes.map((node) => node.id),
            ...(get().editSession?.ops
              .filter((op) => op.op === 'spawn_node')
              .map((op) => op.id) ?? []),
          ],
          sourceNodeId: nodeId,
          sourceTitle: sourceNode.title || nodeId,
          sourceX: sourceNode.position.x,
          sourceY: sourceNode.position.y,
          sourceWidth: graphNodeWidth(sourceNode),
          kind: request.kind,
          storageUri: uploaded.storageUri,
          size: fitMediaNodeSize(uploaded.width, uploaded.height),
        });
      const resultNode = proposal.ops.find((op) => op.op === 'spawn_node');
      if (!resultNode || resultNode.op !== 'spawn_node') {
        throw new Error('静图结果没有生成画布节点');
      }
      await get().appendManualEdit(proposal);
      if (!requestScope.isActive(generation, workspaceId)) {
        throw new WorkspaceChangedError('workspace changed while creating the image result node');
      }
      durableJob = await linkImageProcessingResult(workspaceId, durableJob.id, {
        resultNodeId: resultNode.id,
        outputUploadId: uploaded.uploadId,
      }, signal);
      set((current) => ({
        state: current.state?.workspace.id === workspaceId
          ? withImageProcessingJob(current.state, durableJob as ImageProcessingJob)
          : current.state,
      }));
    } catch (error) {
      const message = error instanceof Error ? error.message : `${label}失败`;
      if (error instanceof WorkspaceChangedError || signal?.aborted) {
        throw error instanceof WorkspaceChangedError
          ? error
          : new WorkspaceChangedError('workspace changed while processing the image');
      }
      const finalError = error instanceof Error ? error : new Error(message);
      set((current) => ({
        state: current.state?.workspace.id === workspaceId
          ? appendSystemError(current.state, finalError.message)
          : current.state,
      }));
      throw finalError;
    }
  };
}

function withImageProcessingJob(
  state: WorkbenchState,
  job: ImageProcessingJob,
): WorkbenchState {
  return {
    ...state,
    imageProcessingJobs: [
      job,
      ...(state.imageProcessingJobs ?? []).filter((item) => item.id !== job.id),
    ],
  };
}

function processorRequest(
  request: ImageCanvasToolRequest,
  source: Blob,
): {
  parameters: Record<string, unknown>;
  source: Blob;
  profile?: string;
} {
  if (request.kind === 'outpaint') {
    return {
      profile: request.profile,
      source,
      parameters: {
        left: request.left,
        top: request.top,
        right: request.right,
        bottom: request.bottom,
        prompt: request.prompt,
        ...(request.quality ? { quality: request.quality } : {}),
        ...(request.sizeTier ? { sizeTier: request.sizeTier } : {}),
      },
    };
  }
  if (request.kind === 'inpaint') {
    return {
      profile: request.profile,
      source: request.maskBlob ?? source,
      parameters: request.maskBlob
        ? {
            prompt: request.prompt,
            ...(request.quality ? { quality: request.quality } : {}),
            ...(request.sizeTier ? { sizeTier: request.sizeTier } : {}),
          }
        : {
            x: request.x,
            y: request.y,
            width: request.width,
            height: request.height,
            prompt: request.prompt,
            ...(request.quality ? { quality: request.quality } : {}),
            ...(request.sizeTier ? { sizeTier: request.sizeTier } : {}),
          },
    };
  }
  if (request.kind === 'upscale') {
    return {
      source,
      parameters: { scale: request.scale, facePolicy: 'off' },
    };
  }
  if (request.kind === 'enhance') {
    return { source, parameters: {} };
  }
  return { source, parameters: {} };
}

export function createGenerateFromMediaCardAction(
  set: StoreApi<WorkbenchStore>['setState'],
  get: StoreApi<WorkbenchStore>['getState'],
  requestScope: WorkspaceRequestScope,
): (nodeId: string, prompt: string, aspectRatio: string) => Promise<void> {
  return async (nodeId, prompt, aspectRatio) => {
    const trimmed = prompt.trim();
    if (!trimmed) throw new Error('请填写提示词');
    const state = get().state;
    if (!state) throw new Error('workspace is not loaded');
    const workspaceId = state.workspace.id;
    const generation = requestScope.currentGeneration();
    const sourceNode = state.graph.nodes.find((node) => node.id === nodeId);
    const workflowNode = state.workflowGraph?.nodes[nodeId];
    if (!sourceNode) throw new Error('选中节点不存在');
    try {
      const source = resolveGridSplitSource({
        nodeType: sourceNode.nodeType,
        params: workflowNode?.params,
        artifacts: artifactsForNode(state.outputs, nodeId),
      });
      const hasImage = Boolean(source);
      const capabilityId = hasImage ? 'image_edit' : 'text_to_image';
      const semantics = await pinnedSemanticsForGenerate(
        workspaceId,
        capabilityId,
        requestScope.signal(),
      );
      await get().appendManualEdit(
        buildMediaGenerateProposalInput({
          baseVersionId: state.workspace.versionId,
          existingNodeIds: [
            ...state.graph.nodes.map((node) => node.id),
            ...(get().editSession?.ops
              .filter((op) => op.op === 'spawn_node')
              .map((op) => op.id) ?? []),
          ],
          sourceNodeId: nodeId,
          sourceTitle: sourceNode.title || nodeId,
          sourceX: sourceNode.position.x,
          sourceY: sourceNode.position.y,
          sourceWidth: graphNodeWidth(sourceNode),
          prompt: trimmed,
          aspectRatio,
          hasImage,
          semantics,
        }),
      );
      if (!requestScope.isActive(generation, workspaceId)) {
        throw new WorkspaceChangedError('workspace changed while creating the generate node');
      }
      await get().queueRun();
    } catch (error) {
      if (error instanceof WorkspaceChangedError) throw error;
      const message = error instanceof Error ? error.message : '生成失败';
      set((current) => ({
        state: current.state?.workspace.id === workspaceId
          ? appendSystemError(current.state, message)
          : current.state,
      }));
      throw error instanceof Error ? error : new Error(message);
    }
  };
}

async function pinnedSemanticsForGenerate(
  workspaceId: string,
  capabilityId: string,
  signal?: AbortSignal,
): Promise<unknown> {
  const requestedModel = useModelPreferenceStore.getState().preferredModelId?.trim();
  if (!requestedModel) return undefined;
  const resolution = await resolveImplementation(workspaceId, capabilityId, requestedModel, signal);
  if (resolution.status !== 'resolved') {
    throw new Error(resolution.message);
  }
  return {
    capabilityId: resolution.resolved.capabilityId,
    mode: capabilityId,
    implementation: {
      pinned: {
        requestedModelId: resolution.resolved.resolvedModelId,
        bindingId: resolution.resolved.bindingId,
      },
    },
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
