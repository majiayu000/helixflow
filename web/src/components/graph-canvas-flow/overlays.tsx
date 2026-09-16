import { useEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react';
import {
  artifactsForNode,
  clampPixelCrop,
  fetchArtifactBlob,
  fetchWorkspaceUploadContent,
  isVisualMediaCardType,
  resolveGridSplitSource,
  TAPNOW_SPLIT_PRESETS,
  type PixelCropRect,
} from '../../grid-split';
import {
  defaultImageCanvasToolRequest,
  imageCanvasToolLabel,
  type ImageCanvasToolRequest,
} from '../../image-canvas-tools';
import { useWorkbenchStore } from '../../store';
import type { AlignKind } from '../graph-canvas-align';
import { CanvasBrowsePanel, buildTemplateOps, type BrowseTab } from './canvas-browse-panel';
import { EraseStage, OutpaintFrame, TextCardEditor } from './canvas-image-modes';
import { downloadCardMedia, saveCardAsset } from './canvas-card-media';
import { EmptyCardUpload, MediaCardComposer, mentionItemsFromNodes } from './media-card-overlays';
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
import {
  graphNodeWidth,
  type ViewState,
  type ViewportSize,
} from '../graph-canvas-navigation';
import type { GraphCanvasProps } from '../graph-canvas-types';
import { NodeLibrary } from '../node-library';
import { CanvasStatusToast } from './controls';

type Readiness = NonNullable<WorkbenchState['providers']['capabilityReadiness']>[number];
type EditActions = ReturnType<typeof createCanvasEditActions>;

export type CanvasAddMenu = {
  clientX: number;
  clientY: number;
  flow: { x: number; y: number };
  connectFrom?: { nodeId: string; handleType: 'source' | 'target' };
};

type CanvasOverlaysProps = Pick<
  GraphCanvasProps,
  | 'comments'
  | 'onCommentOp'
  | 'onRequestNodeProposal'
  | 'onSetParam'
  | 'onStartFromPrompt'
  | 'outputs'
  | 'providers'
  | 'workspaceId'
> & {
  addMenu: CanvasAddMenu | null;
  canEdit: boolean;
  chromeHidden?: boolean;
  onAlign?: (kind: AlignKind) => void;
  onGroup?: () => void;
  onUngroup?: () => void;
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
  onConnectReference?: (sourceId: string, targetId: string) => void;
  onJumpNode?: (nodeId: string) => void;
  onRedo?: () => void;
  onStartReferencePicker?: (nodeId: string) => void;
  onUndo?: () => void;
  referencePickerNodeId?: string | null;
  editingTextNodeId: string | null;
  selectedIds: Set<string>;
  selectedNodes: WorkbenchState['graph']['nodes'];
  setAddMenu: (menu: CanvasAddMenu | null) => void;
  setEditStatus: (value: string | null) => void;
  setSelection: (ids: Iterable<string>) => void;
  view: ViewState;
  viewportSize: ViewportSize;
  workflowGraph?: WorkbenchState['workflowGraph'];
};

export function CanvasOverlays(props: CanvasOverlaysProps) {
  const selectedNode = props.selectedNodes.length === 1 ? props.selectedNodes[0] : undefined;
  const selectedNodeId = selectedNode?.id;
  const applyImageCanvasTool = useWorkbenchStore((state) => state.applyImageCanvasTool);
  const splitImageGrid = useWorkbenchStore((state) => state.splitImageGrid);
  const ingestMediaFiles = useWorkbenchStore((state) => state.ingestMediaFiles);
  const cropImageNode = useWorkbenchStore((state) => state.cropImageNode);
  const replaceNodeMedia = useWorkbenchStore((state) => state.replaceNodeMedia);
  const [cropOpen, setCropOpen] = useState(false);
  const [splitOpen, setSplitOpen] = useState(false);
  const [scaleOpen, setScaleOpen] = useState(false);
  const [imageMode, setImageMode] = useState<'outpaint' | 'erase' | null>(null);
  const [browseTab, setBrowseTab] = useState<BrowseTab | null>(null);
  useEffect(() => {
    setImageMode(null);
  }, [selectedNodeId]);
  const hasImageSource = Boolean(
    selectedNode &&
      resolveGridSplitSource({
        nodeType: selectedNode.nodeType,
        params: props.workflowGraph?.nodes[selectedNode.id]?.params,
        artifacts: artifactsForNode(props.outputs, selectedNode.id),
      }),
  );
  const runImageTool = async (nodeId: string, request: ImageCanvasToolRequest) => {
    const label = imageCanvasToolLabel(request.kind);
    props.setEditStatus(`${label}处理中…`);
    try {
      await applyImageCanvasTool(nodeId, request);
      props.setEditStatus(`${label}完成`);
    } catch (error) {
      props.setEditStatus(error instanceof Error ? error.message : `${label}失败`);
    }
  };
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
        onOpenBrowse={props.canEdit ? setBrowseTab : undefined}
        onRedo={props.onRedo}
        onUndo={props.onUndo}
        onUploadFiles={props.canEdit ? (files) => void ingestMediaFiles(files) : undefined}
      />
      {props.referencePickerNodeId ? (
        <div className="canvas-ref-banner">点一张图当参考 · Esc 取消</div>
      ) : null}
      {props.drawGraph.nodes.length === 0 && (
        <div className="canvas-drop-hint" aria-hidden="true">
          <strong>把图片拖进画布</strong>
          <span>也可以粘贴，或点左下角上传</span>
        </div>
      )}
      <CanvasStatusToast
        editStatus={props.editStatus}
        pendingProposal={props.pendingProposal}
      />
      {!props.chromeHidden && props.selectedNodes.length > 1 && (
        <GraphSelectionInspector
          nodes={props.selectedNodes}
          onAlign={props.onAlign}
          onClose={() => props.setSelection([])}
          onCopy={() => void props.editActions.copySelection(props.selectedNodes)}
          onDelete={() => props.editActions.deleteSelection(props.selectedIds)}
          onGroup={props.onGroup}
          onUngroup={props.onUngroup}
          view={props.view}
          viewportSize={props.viewportSize}
        />
      )}
      {!props.chromeHidden && selectedNode && !isVisualMediaCardType(selectedNode.nodeType) && selectedNode.nodeType !== 'input.text' && (
        <GraphInspector
          catalogError={props.catalogError}
          definition={props.definitionByType.get(selectedNode.nodeType)}
          node={selectedNode}
          resolution={props.resolution}
          readiness={props.readiness}
          onClose={() => props.setSelection([])}
          onRequestProposal={props.canEdit ? props.onRequestNodeProposal : undefined}
          onSetParam={props.canEdit ? props.onSetParam : undefined}
          onApplyImageCanvasTool={
            props.canEdit
              ? (request) => runImageTool(selectedNode.id, request)
              : undefined
          }
          onSplitImageGrid={
            props.canEdit
              ? (rows, columns) => splitImageGrid(selectedNode.id, rows, columns)
              : undefined
          }
          view={props.view}
          viewportSize={props.viewportSize}
          workspaceId={props.workspaceId}
          workflowNode={props.workflowGraph?.nodes[selectedNode.id]}
        />
      )}
      {!props.chromeHidden && props.canEdit && selectedNode && isVisualMediaCardType(selectedNode.nodeType) && (
        <>
          {!hasImageSource ? (
            <EmptyCardUpload
              node={selectedNode}
              view={props.view}
              onUpload={(file) => void replaceNodeMedia(selectedNode.id, file)}
            />
          ) : (
          <ImageCardToolbar
            hasImage={hasImageSource}
            node={selectedNode}
            splitOpen={splitOpen}
            view={props.view}
            onCrop={() => setCropOpen(true)}
            onEnhance={() => void runImageTool(selectedNode.id, defaultImageCanvasToolRequest('enhance'))}
            onErase={() => setImageMode('erase')}
            onOutpaint={() => setImageMode('outpaint')}
            onScaleOpen={() => setScaleOpen((open) => !open)}
            onUpscale={(scale) => {
              setScaleOpen(false);
              void runImageTool(selectedNode.id, {
                kind: 'upscale',
                scale,
              });
            }}
            scaleOpen={scaleOpen}
            onDelete={() => props.editActions.deleteSelection([selectedNode.id])}
            onDownload={() => {
              if (!props.workspaceId) return;
              void downloadCardMedia({
                node: selectedNode,
                outputs: props.outputs,
                params: props.workflowGraph?.nodes[selectedNode.id]?.params,
                workspaceId: props.workspaceId,
              }).catch((error) => {
                props.setEditStatus(error instanceof Error ? error.message : '下载失败');
              });
            }}
            onDuplicate={() => props.editActions.duplicateNode(selectedNode)}
            onSaveAsset={() => {
              if (!props.workspaceId) return;
              void saveCardAsset({
                node: selectedNode,
                outputs: props.outputs,
                params: props.workflowGraph?.nodes[selectedNode.id]?.params,
                workspaceId: props.workspaceId,
              }).then(() => {
                props.setEditStatus('已存进素材库');
              }).catch((error) => {
                props.setEditStatus(error instanceof Error ? error.message : '存素材失败');
              });
            }}
            onReplace={(file) => void replaceNodeMedia(selectedNode.id, file)}
            onSplit={(rows, columns) => {
              setSplitOpen(false);
              void splitImageGrid(selectedNode.id, rows, columns);
            }}
            onSplitOpen={() => setSplitOpen((open) => !open)}
            onTool={(kind) => void runImageTool(selectedNode.id, defaultImageCanvasToolRequest(kind))}
          />
          )}
          {!imageMode && (selectedNode.nodeType === 'input.image' ||
            selectedNode.nodeType === 'image.generate' ||
            selectedNode.nodeType === 'image.edit') && (
            <MediaCardComposer
              mentionItems={mentionItemsFromNodes(props.drawGraph.nodes, selectedNode.id)}
              modelCatalog={props.modelCatalog}
              node={selectedNode}
              pickingReference={props.referencePickerNodeId === selectedNode.id}
              providers={props.providers}
              view={props.view}
              workflowPrompt={stringParam(props.workflowGraph?.nodes[selectedNode.id]?.params, 'prompt')}
              onMention={(item) => props.onConnectReference?.(item.id, selectedNode.id)}
              onPickReference={() => props.onStartReferencePicker?.(selectedNode.id)}
            />
          )}
        </>
      )}
      {props.canEdit && selectedNode?.nodeType === 'input.text' && props.editingTextNodeId === selectedNode.id && (
        <TextCardEditor
          key={selectedNode.id}
          node={selectedNode}
          view={props.view}
          value={stringParam(props.workflowGraph?.nodes[selectedNode.id]?.params, 'text')}
          onSave={(text) => {
            if (props.onSetParam) void props.onSetParam(selectedNode.id, 'text', text);
          }}
        />
      )}
      {imageMode === 'outpaint' && selectedNode && props.workspaceId && (
        <OutpaintFrame
          node={selectedNode}
          outputs={props.outputs}
          params={props.workflowGraph?.nodes[selectedNode.id]?.params}
          view={props.view}
          workspaceId={props.workspaceId}
          onCancel={() => setImageMode(null)}
          onConfirm={(pad, options) => {
            setImageMode(null);
            void runImageTool(selectedNode.id, {
              kind: 'outpaint',
              ...pad,
              ...options,
            });
          }}
        />
      )}
      {imageMode === 'erase' && selectedNode && props.workspaceId && (
        <EraseStage
          node={selectedNode}
          outputs={props.outputs}
          params={props.workflowGraph?.nodes[selectedNode.id]?.params}
          view={props.view}
          workspaceId={props.workspaceId}
          onCancel={() => setImageMode(null)}
          onConfirm={(maskBlob, options) => {
            setImageMode(null);
            void runImageTool(selectedNode.id, {
              kind: 'inpaint',
              x: 0,
              y: 0,
              width: 1,
              height: 1,
              ...options,
              maskBlob,
            });
          }}
        />
      )}
      {browseTab && (
        <CanvasBrowsePanel
          nodes={props.drawGraph.nodes}
          outputs={props.outputs}
          tab={browseTab}
          workflowGraph={props.workflowGraph}
          onClose={() => setBrowseTab(null)}
          onDuplicate={(node) => props.editActions.duplicateNode(node)}
          onPlaceFile={(file) => void ingestMediaFiles(file)}
          onJump={(nodeId) => {
            props.onJumpNode?.(nodeId);
            setBrowseTab(null);
          }}
          onTemplate={(id) => {
            const origin = {
              x: (props.viewportSize.width / 2 - props.view.x) / props.view.z,
              y: (props.viewportSize.height / 2 - props.view.y) / props.view.z,
            };
            for (const item of buildTemplateOps(id, props.definitionByType, origin)) {
              props.editActions.addNode(item.definition, item.position);
            }
            setBrowseTab(null);
          }}
        />
      )}
      {cropOpen && selectedNode && props.workspaceId && (
        <PixelCropDialog
          nodeId={selectedNode.id}
          nodeType={selectedNode.nodeType}
          params={props.workflowGraph?.nodes[selectedNode.id]?.params}
          outputs={props.outputs}
          workspaceId={props.workspaceId}
          onClose={() => setCropOpen(false)}
          onConfirm={(crop) => {
            setCropOpen(false);
            void cropImageNode(selectedNode.id, crop);
          }}
        />
      )}
      {props.canEdit && props.addMenu && (
        <CanvasAddMenuPanel
          definitions={props.definitionByType}
          menu={props.addMenu}
          view={props.view}
          onAdd={(definition) => {
            const source = props.addMenu?.connectFrom
              ? props.drawGraph.nodes.find((node) => node.id === props.addMenu?.connectFrom?.nodeId)
              : undefined;
            const sourceDefinition = source
              ? props.definitionByType.get(source.nodeType)
              : undefined;
            const inbound = props.addMenu?.connectFrom?.handleType === 'target';
            const position = source
              ? {
                  x: inbound
                    ? source.position.x - graphNodeWidth(source) - 48
                    : source.position.x + graphNodeWidth(source) + 48,
                  y: source.position.y,
                }
              : props.addMenu!.flow;
            props.editActions.addNode(
              definition,
              position,
              source && sourceDefinition
                ? {
                    nodeId: source.id,
                    definition: sourceDefinition,
                    direction: inbound ? 'in' : 'out',
                  }
                : undefined,
            );
            props.setAddMenu(null);
          }}
          onClose={() => props.setAddMenu(null)}
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

function ImageCardToolbar({
  hasImage,
  node,
  scaleOpen,
  splitOpen,
  view,
  onCrop,
  onDelete,
  onDownload,
  onDuplicate,
  onEnhance,
  onErase,
  onOutpaint,
  onReplace,
  onSaveAsset,
  onScaleOpen,
  onSplit,
  onSplitOpen,
  onTool,
  onUpscale,
}: {
  hasImage: boolean;
  node: WorkbenchState['graph']['nodes'][number];
  scaleOpen: boolean;
  splitOpen: boolean;
  view: ViewState;
  onCrop: () => void;
  onDelete: () => void;
  onDownload: () => void;
  onDuplicate: () => void;
  onEnhance: () => void;
  onErase: () => void;
  onOutpaint: () => void;
  onReplace: (file: File) => void;
  onSaveAsset: () => void;
  onScaleOpen: () => void;
  onSplit: (rows: number, columns: number) => void;
  onSplitOpen: () => void;
  onTool: (kind: 'cutout') => void;
  onUpscale: (scale: 2 | 4) => void;
}) {
  const replaceRef = useRef<HTMLInputElement | null>(null);
  const left = node.position.x * view.z + view.x + (graphNodeWidth(node) * view.z) / 2;
  const top = node.position.y * view.z + view.y - 10;
  return (
    <div
      className="canvas-image-toolbar"
      style={{ left, top } as CSSProperties}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <button disabled={!hasImage} onClick={onCrop} type="button">裁剪</button>
      <div className="canvas-image-toolbar-split">
        <button disabled={!hasImage} onClick={onSplitOpen} type="button">切分</button>
        {splitOpen && (
          <div className="canvas-image-toolbar-menu">
            {TAPNOW_SPLIT_PRESETS.map((preset) => (
              <button
                key={preset.label}
                onClick={() => onSplit(preset.rows, preset.columns)}
                type="button"
              >
                {preset.label}
              </button>
            ))}
          </div>
        )}
      </div>
      <button disabled={!hasImage} onClick={onOutpaint} type="button">扩图</button>
      <button disabled={!hasImage} onClick={onErase} type="button">擦除</button>
      <button disabled={!hasImage} onClick={() => onTool('cutout')} type="button">抠图</button>
      <div className="canvas-image-toolbar-split">
        <button disabled={!hasImage} onClick={onScaleOpen} type="button">2×</button>
        {scaleOpen && (
          <div className="canvas-image-toolbar-menu">
            <button onClick={() => onUpscale(2)} type="button">2×</button>
            <button onClick={() => onUpscale(4)} type="button">4×</button>
          </div>
        )}
      </div>
      <button disabled={!hasImage} onClick={onEnhance} type="button">增强</button>
      <button disabled={!hasImage} onClick={onSaveAsset} type="button">存素材</button>
      {hasImage ? (
        <button onClick={() => replaceRef.current?.click()} type="button">替换</button>
      ) : null}
      <button onClick={onDuplicate} type="button">复制</button>
      <button disabled={!hasImage} onClick={onDownload} type="button">下载</button>
      <button onClick={onDelete} type="button">删除</button>
      <input
        accept="image/*,video/*,audio/*"
        hidden
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          event.currentTarget.value = '';
          if (file) onReplace(file);
        }}
        ref={replaceRef}
        type="file"
      />
    </div>
  );
}

function PixelCropDialog({
  nodeId,
  nodeType,
  params,
  outputs,
  workspaceId,
  onClose,
  onConfirm,
}: {
  nodeId: string;
  nodeType: string;
  params: unknown;
  outputs: WorkbenchState['outputs'] | undefined;
  workspaceId: string;
  onClose: () => void;
  onConfirm: (crop: PixelCropRect) => void;
}) {
  const [url, setUrl] = useState<string | null>(null);
  const [natural, setNatural] = useState({ width: 0, height: 0 });
  const [crop, setCrop] = useState<PixelCropRect>({ x: 0, y: 0, width: 1, height: 1 });
  const drag = useRef<{ pointerId: number; startX: number; startY: number; origin: PixelCropRect } | null>(null);

  useEffect(() => {
    const source = resolveGridSplitSource({
      nodeType,
      params,
      artifacts: artifactsForNode(outputs, nodeId),
    });
    if (!source) return;
    let cancelled = false;
    let objectUrl: string | null = null;
    const blobPromise = source.kind === 'upload'
      ? fetchWorkspaceUploadContent(workspaceId, source.uploadId)
      : fetchArtifactBlob(source.artifactId);
    void blobPromise.then(async (blob) => {
      objectUrl = URL.createObjectURL(blob);
      const bitmap = await createImageBitmap(blob);
      if (cancelled) {
        bitmap.close();
        return;
      }
      setNatural({ width: bitmap.width, height: bitmap.height });
      setCrop({
        x: Math.round(bitmap.width * 0.1),
        y: Math.round(bitmap.height * 0.1),
        width: Math.round(bitmap.width * 0.8),
        height: Math.round(bitmap.height * 0.8),
      });
      setUrl(objectUrl);
      bitmap.close();
    });
    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [nodeId, nodeType, outputs, params, workspaceId]);

  const applyAspect = (ratio: number | null) => {
    if (!natural.width) return;
    if (ratio == null) {
      setCrop({ x: 0, y: 0, width: natural.width, height: natural.height });
      return;
    }
    let width = natural.width;
    let height = Math.round(width / ratio);
    if (height > natural.height) {
      height = natural.height;
      width = Math.round(height * ratio);
    }
    setCrop(clampPixelCrop({
      x: Math.round((natural.width - width) / 2),
      y: Math.round((natural.height - height) / 2),
      width,
      height,
    }, natural.width, natural.height));
  };

  const startDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      origin: crop,
    };
  };
  const moveDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag.current || drag.current.pointerId !== event.pointerId || !natural.width) return;
    const scale = 360 / natural.width;
    setCrop(clampPixelCrop({
      ...drag.current.origin,
      x: drag.current.origin.x + (event.clientX - drag.current.startX) / scale,
      y: drag.current.origin.y + (event.clientY - drag.current.startY) / scale,
    }, natural.width, natural.height));
  };

  return (
    <div className="pixel-crop-dialog" onPointerDown={(event) => event.stopPropagation()}>
      <div className="pixel-crop-card">
        <div className="pixel-crop-head">
          <strong>裁剪</strong>
          <span>源图保留，结果出现在右侧</span>
        </div>
        <div className="pixel-crop-stage">
          {url ? (
            <div
              className="pixel-crop-image"
              onPointerDown={startDrag}
              onPointerMove={moveDrag}
              onPointerUp={() => { drag.current = null; }}
              style={{ aspectRatio: `${natural.width} / ${Math.max(1, natural.height)}` }}
            >
              <img alt="" src={url} />
              <span
                className="pixel-crop-rect"
                style={{
                  left: `${(crop.x / Math.max(1, natural.width)) * 100}%`,
                  top: `${(crop.y / Math.max(1, natural.height)) * 100}%`,
                  width: `${(crop.width / Math.max(1, natural.width)) * 100}%`,
                  height: `${(crop.height / Math.max(1, natural.height)) * 100}%`,
                }}
              />
            </div>
          ) : (
            <div className="pixel-crop-empty">正在读取原图…</div>
          )}
        </div>
        <div className="pixel-crop-aspects">
          <button onClick={() => applyAspect(null)} type="button">原图</button>
          <button onClick={() => applyAspect(1)} type="button">1:1</button>
          <button onClick={() => applyAspect(16 / 9)} type="button">16:9</button>
          <button onClick={() => applyAspect(9 / 16)} type="button">9:16</button>
          <button onClick={() => applyAspect(4 / 3)} type="button">4:3</button>
        </div>
        <div className="pixel-crop-actions">
          <button onClick={onClose} type="button">取消</button>
          <button
            className="pixel-crop-confirm"
            disabled={!url}
            onClick={() => onConfirm(crop)}
            type="button"
          >
            确认裁剪
          </button>
        </div>
      </div>
    </div>
  );
}

function stringParam(params: unknown, key: string): string {
  if (!params || typeof params !== 'object' || Array.isArray(params)) return '';
  const value = (params as Record<string, unknown>)[key];
  return typeof value === 'string' ? value : '';
}

function CanvasAddMenuPanel({
  definitions,
  menu,
  view,
  onAdd,
  onClose,
}: {
  definitions: Map<string, NodeDefinition>;
  menu: CanvasAddMenu;
  view: ViewState;
  onAdd: (definition: NodeDefinition) => void;
  onClose: () => void;
}) {
  const items = [
    ['input.text', '文本'],
    ['input.image', '图片'],
    ['input.video', '视频'],
    ['input.audio', '音频'],
  ] as const;
  return (
    <div
      className="canvas-add-menu"
      style={{
        left: menu.flow.x * view.z + view.x,
        top: menu.flow.y * view.z + view.y,
      } as CSSProperties}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <div className="canvas-add-menu-head">添加节点</div>
      {items.map(([type, label]) => {
        const definition = definitions.get(type);
        if (!definition) return null;
        return (
          <button key={type} onClick={() => onAdd(definition)} type="button">
            {label}
          </button>
        );
      })}
      <button className="canvas-add-menu-cancel" onClick={onClose} type="button">
        取消
      </button>
    </div>
  );
}
