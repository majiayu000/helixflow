import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import {
  artifactsForNode,
  clampPixelCrop,
  fetchArtifactBlob,
  fetchWorkspaceUploadContent,
  isVisualMediaCardType,
  parseUploadUri,
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
import { EraseStage, OutpaintFrame, TextCardEditor, CameraToolPanel } from './canvas-image-modes';
import { downloadCardMedia, saveCardAsset } from './canvas-card-media';
import { EmptyCardUpload, MediaCardComposer, mentionItemsFromNodes } from './media-card-overlays';
import type {
  ImplementationResolution,
  ModelCatalog,
  NodeCatalog,
  NodeDefinition,
  WorkbenchState,
} from '../../types';
import { fanInConnectFrom, mediaGenerateTarget } from '../graph-canvas-editing';
import { CanvasAddMenuPanel, type CanvasAddMenu } from './canvas-add-menu';
import { flowAboveCenterStyle } from './overlay-anchor';
import { MediaCardToolbar } from './media-card-toolbar';
import { CanvasCommentsPanel, CommentComposePin } from '../graph-canvas-collaboration';
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
  browseTab?: BrowseTab | null;
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
  pointerWorld?: () => { x: number; y: number };
  readiness?: Readiness;
  resolution: ImplementationResolution | null;
  onBrowseTab?: (tab: BrowseTab | null) => void;
  onConnectReference?: (sourceId: string, targetId: string) => void;
  onJumpNode?: (nodeId: string) => void;
  onPasteMedia?: () => Promise<void>;
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
  cardOverlayHost?: HTMLElement | null;
  commentMode?: boolean;
  commentDraftAt?: { x: number; y: number } | null;
  onCommentModeChange?: (open: boolean) => void;
  onCommentDraftAt?: (point: { x: number; y: number } | null) => void;
};

export function CanvasOverlays(props: CanvasOverlaysProps) {
  const selectedNode = props.selectedNodes.length === 1 ? props.selectedNodes[0] : undefined;
  const selectedNodeId = selectedNode?.id;
  const applyImageCanvasTool = useWorkbenchStore((state) => state.applyImageCanvasTool);
  const generateFromMediaCard = useWorkbenchStore((state) => state.generateFromMediaCard);
  const splitImageGrid = useWorkbenchStore((state) => state.splitImageGrid);
  const ingestMediaFiles = useWorkbenchStore((state) => state.ingestMediaFiles);
  const cropImageNode = useWorkbenchStore((state) => state.cropImageNode);
  const extractVideoFrame = useWorkbenchStore((state) => state.extractVideoFrame);
  const replaceNodeMedia = useWorkbenchStore((state) => state.replaceNodeMedia);
  const [cropOpen, setCropOpen] = useState(false);
  const [splitOpen, setSplitOpen] = useState(false);
  const [scaleOpen, setScaleOpen] = useState(false);
  const [imageMode, setImageMode] = useState<'outpaint' | 'erase' | 'redraw' | 'relight' | 'multi-angle' | null>(null);
  const [localBrowseTab, setLocalBrowseTab] = useState<BrowseTab | null>(null);
  const [commentsOpen, setCommentsOpen] = useState(false);
  const browseTab = props.browseTab === undefined ? localBrowseTab : props.browseTab;
  const setBrowseTab = props.onBrowseTab ?? setLocalBrowseTab;
  useEffect(() => {
    setImageMode(null);
  }, [selectedNodeId]);
  const selectedParams = selectedNode ? props.workflowGraph?.nodes[selectedNode.id]?.params : undefined;
  const selectedArtifacts = selectedNode ? artifactsForNode(props.outputs, selectedNode.id) : [];
  const hasImageSource = Boolean(
    selectedNode &&
      resolveGridSplitSource({
        nodeType: selectedNode.nodeType,
        params: selectedParams,
        artifacts: selectedArtifacts,
      }),
  );
  const hasMediaSource = Boolean(
    hasImageSource
    || parseUploadUri(stringParam(selectedParams, 'storage_uri'))
    || selectedArtifacts.some((item) => item.kind === 'image' || item.kind === 'video' || item.kind === 'audio'),
  );
  const showComposer = Boolean(
    selectedNode &&
      props.canEdit &&
      !imageMode &&
      (isVisualMediaCardType(selectedNode.nodeType) || selectedNode.nodeType === 'input.text') &&
      mediaGenerateTarget({
        nodeType: selectedNode.nodeType,
        hasImage: hasImageSource,
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
        onAddNode={(definition) => {
          props.editActions.addNode(
            definition,
            undefined,
            fanInConnectFrom(props.selectedIds, props.drawGraph.nodes, props.definitionByType),
          );
        }}
        onOpenBrowse={props.canEdit ? setBrowseTab : undefined}
        onOpenComments={
          props.canEdit
            ? () => {
                const next = !props.commentMode;
                props.onCommentModeChange?.(next);
                if (!next) props.onCommentDraftAt?.(null);
              }
            : undefined
        }
        commentMode={props.commentMode}
        onRedo={props.onRedo}
        onUndo={props.onUndo}
        onUploadFiles={props.canEdit ? (files) => void ingestMediaFiles(files, props.pointerWorld?.()) : undefined}
      />
      {props.commentMode ? (
        <div className="canvas-ref-banner">点空白处添加评论 · Esc 退出</div>
      ) : null}
      {props.referencePickerNodeId ? (
        <div className="canvas-ref-banner">点一张图当参考 · Esc 取消</div>
      ) : null}
      {props.drawGraph.nodes.length === 0 && (
        <div className="canvas-empty">
          <span className="canvas-empty-badge">双击</span>
          <strong>把图片拖进画布</strong>
          <span>也可以粘贴，或点左侧 +、空白处双击 / 右键添加节点</span>
          {props.onStartFromPrompt && props.canEdit && (
            <div className="canvas-empty-recipes">
              <button type="button" onClick={() => props.onStartFromPrompt?.('用文字生成一段产品视频')}>文字生视频</button>
              <button type="button" onClick={() => props.onStartFromPrompt?.('把产品图换成新的场景背景')}>图片换背景</button>
              <button type="button" onClick={() => props.onStartFromPrompt?.('用首帧图片生成视频')}>首帧生成视频</button>
              <button type="button" onClick={() => props.onStartFromPrompt?.('用音频驱动生成视频')}>音频生视频</button>
            </div>
          )}
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
      {maybePortal(props.cardOverlayHost, (
        <>
      {props.canEdit && selectedNode && isVisualMediaCardType(selectedNode.nodeType) && (
        <>
          {!hasMediaSource ? (
            <EmptyCardUpload
              node={selectedNode}
              view={props.view}
              onUpload={(file) => void replaceNodeMedia(selectedNode.id, file)}
            />
          ) : hasImageSource && (selectedNode.nodeType === 'input.image' ||
            selectedNode.nodeType === 'image.generate' ||
            selectedNode.nodeType === 'image.edit') ? (
          <ImageCardToolbar
            hasImage={hasImageSource}
            node={selectedNode}
            splitOpen={splitOpen}
            view={props.view}
            onCrop={() => setCropOpen(true)}
            onErase={() => setImageMode('erase')}
            onOutpaint={() => setImageMode('outpaint')}
            onRelight={() => setImageMode('relight')}
            onMultiAngle={() => setImageMode('multi-angle')}
            onRedraw={() => setImageMode('redraw')}
            onEnhance={() => void runImageTool(selectedNode.id, defaultImageCanvasToolRequest('enhance'))}
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
          ) : (
            <MediaCardToolbar
              canDownload={hasMediaSource}
              node={selectedNode}
              view={props.view}
              onDelete={() => props.editActions.deleteSelection([selectedNode.id])}
              onExtractFrame={
                selectedNode.nodeType === 'input.video' || selectedNode.nodeType.startsWith('video.')
                  ? (kind) => {
                      props.setEditStatus('抽帧中…');
                      void extractVideoFrame(selectedNode.id, kind)
                        .then(() => props.setEditStatus('已抽出图片卡'))
                        .catch((error) => {
                          props.setEditStatus(error instanceof Error ? error.message : '抽帧失败');
                        });
                    }
                  : undefined
              }
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
              onReplace={(file) => void replaceNodeMedia(selectedNode.id, file)}
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
            />
          )}
        </>
      )}
      {showComposer && selectedNode && (
            <MediaCardComposer
              hasSource={hasImageSource || hasMediaSource}
              mentionItems={mentionItemsFromNodes(props.drawGraph.nodes, selectedNode.id)}
              modelCatalog={props.modelCatalog}
              node={selectedNode}
              pickingReference={props.referencePickerNodeId === selectedNode.id}
              providers={props.providers}
              view={props.view}
              workflowPrompt={stringParam(
                props.workflowGraph?.nodes[selectedNode.id]?.params,
                selectedNode.nodeType === 'input.text' ? 'text' : 'prompt',
              )}
              onMention={(item) => props.onConnectReference?.(item.id, selectedNode.id)}
              onPickReference={() => props.onStartReferencePicker?.(selectedNode.id)}
            />
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
      {(imageMode === 'erase' || imageMode === 'redraw') && selectedNode && props.workspaceId && (
        <EraseStage
          intent={imageMode}
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
      {(imageMode === 'relight' || imageMode === 'multi-angle') && selectedNode && (
        <CameraToolPanel
          kind={imageMode}
          node={selectedNode}
          view={props.view}
          onCancel={() => setImageMode(null)}
          onRun={(prompt) => {
            setImageMode(null);
            props.setEditStatus('生成中…');
            void generateFromMediaCard(selectedNode.id, prompt, '1:1')
              .then(() => props.setEditStatus('已在右侧生成新卡'))
              .catch((error) => {
                props.setEditStatus(error instanceof Error ? error.message : '生成失败');
              });
          }}
        />
      )}
      {props.commentDraftAt && props.canEdit && props.onCommentOp && (
        <CommentComposePin
          x={props.commentDraftAt.x}
          y={props.commentDraftAt.y}
          onCancel={() => props.onCommentDraftAt?.(null)}
          onSubmit={async (body) => {
            await props.onCommentOp?.({
              op: {
                op: 'comment_add',
                target: { kind: 'position', x: props.commentDraftAt!.x, y: props.commentDraftAt!.y },
                body,
              },
            });
            props.onCommentDraftAt?.(null);
          }}
        />
      )}
        </>
      ))}
      {browseTab && (
        <CanvasBrowsePanel
          nodes={props.drawGraph.nodes}
          outputs={props.outputs}
          tab={browseTab}
          workflowGraph={props.workflowGraph}
          onClose={() => setBrowseTab(null)}
          onDuplicate={(node) => props.editActions.duplicateNode(node)}
          onPlaceFile={(file) => void ingestMediaFiles(file, props.pointerWorld?.())}
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
      {props.canEdit && props.addMenu && !props.chromeHidden && (
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
                    extra: inbound || !props.selectedIds.has(source.id)
                      ? []
                      : fanInConnectFrom(
                          props.selectedIds,
                          props.drawGraph.nodes,
                          props.definitionByType,
                          source.id,
                        )?.extra ?? [],
                  }
                : fanInConnectFrom(
                    props.selectedIds,
                    props.drawGraph.nodes,
                    props.definitionByType,
                  ),
            );
            props.setAddMenu(null);
          }}
          onClose={() => props.setAddMenu(null)}
          onPaste={props.onPasteMedia
            ? () => {
                props.setAddMenu(null);
                void props.onPasteMedia?.();
              }
            : undefined}
          onUpload={(files) => {
            const origin = props.addMenu?.flow;
            props.setAddMenu(null);
            void ingestMediaFiles(files, origin);
          }}
        />
      )}
      <CanvasCommentsPanel
        comments={props.comments ?? []}
        edges={props.drawGraph.edges}
        nodes={props.drawGraph.nodes}
        onCommentOp={props.canEdit ? props.onCommentOp : undefined}
        open={commentsOpen}
        onOpenChange={setCommentsOpen}
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
  view: _view,
  onCrop,
  onDelete,
  onDownload,
  onDuplicate,
  onEnhance,
  onErase,
  onMultiAngle,
  onOutpaint,
  onRedraw,
  onRelight,
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
  onMultiAngle: () => void;
  onOutpaint: () => void;
  onRedraw: () => void;
  onRelight: () => void;
  onReplace: (file: File) => void;
  onSaveAsset: () => void;
  onScaleOpen: () => void;
  onSplit: (rows: number, columns: number) => void;
  onSplitOpen: () => void;
  onTool: (kind: 'cutout') => void;
  onUpscale: (scale: 2 | 4) => void;
}) {
  const replaceRef = useRef<HTMLInputElement | null>(null);
  const [moreOpen, setMoreOpen] = useState(false);
  return (
    <div
      className="canvas-image-toolbar nodrag nopan"
      style={flowAboveCenterStyle(node, 10)}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <button disabled={!hasImage} onClick={onCrop} type="button">裁剪</button>
      <button disabled={!hasImage} onClick={onMultiAngle} type="button">换角度</button>
      <button disabled={!hasImage} onClick={onRedraw} type="button">重绘</button>
      <button disabled={!hasImage} onClick={onRelight} type="button">调光</button>
      <div className="canvas-image-toolbar-split">
        <button disabled={!hasImage} onClick={() => setMoreOpen((open) => !open)} type="button">···</button>
        {moreOpen && (
          <div className="canvas-image-toolbar-menu">
            <button onClick={onOutpaint} type="button">扩图</button>
            <button onClick={onErase} type="button">擦除</button>
            <button onClick={onEnhance} type="button">增强</button>
            <button onClick={() => onTool('cutout')} type="button">抠图</button>
            <button onClick={onScaleOpen} type="button">超分</button>
            <button onClick={onSplitOpen} type="button">快速切分</button>
            <button onClick={onSaveAsset} type="button">入库</button>
          </div>
        )}
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
        {scaleOpen && (
          <div className="canvas-image-toolbar-menu">
            <button onClick={() => onUpscale(2)} type="button">2×</button>
            <button onClick={() => onUpscale(4)} type="button">4×</button>
          </div>
        )}
      </div>
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

function maybePortal(host: HTMLElement | null | undefined, node: ReactNode) {
  return host ? createPortal(node, host) : node;
}
