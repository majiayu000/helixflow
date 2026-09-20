import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from 'react';
import { Icon, Port } from '../icons';
import {
  fetchWorkspaceUploadContent,
  isCanvasCardType,
  isMediaNodeType,
  mediaKindLabel,
  parseUploadUri,
} from '../grid-split';
import type { GraphNodeState, NodeDefinition, RunStepState, WorkflowGraph } from '../types';
import { markdownToHtml } from '../text-card-markdown';
import type { CanvasNodeArtifact } from './graph-canvas-artifacts';
import { portKey, type PortHighlight } from './graph-canvas-connections';
import { graphNodeHeight, graphNodeWidth } from './graph-canvas-navigation';
import {
  categorySwatch,
  paramsFromSummary,
  type DiffState,
} from './graph-canvas-rendering';
import {
  CANVAS_VIDEO_DECODER_PRIORITY,
  useCanvasVideoDecoder,
} from './graph-canvas-video-decoder';

type WorkflowNodeProps = {
  node: GraphNodeState;
  definition?: NodeDefinition;
  workflowNode?: WorkflowGraph['nodes'][string];
  diffState: DiffState;
  dirty: boolean;
  locked: boolean;
  connectionDisabled: boolean;
  portHighlights: Map<string, PortHighlight>;
  selected: boolean;
  stepState: RunStepState;
  stepError?: string | null;
  referenceBagLabel?: string | null;
  artifactOutputs: CanvasNodeArtifact[];
  resizable: boolean;
  embedded?: boolean;
  workspaceId?: string;
  onUploadMedia?: (file: File) => void;
  onSelectOutput?: (outputId: string) => void;
  onKeyboardSelect?: (additive: boolean) => void;
  onOutputPortPointerDown: (
    node: GraphNodeState,
    port: { name: string; type: string },
    index: number,
    event: PointerEvent<HTMLSpanElement>,
  ) => void;
  onPointerCancel: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerDown: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerMove: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerUp: (event: PointerEvent<HTMLDivElement>) => void;
  onResizePointerCancel: (event: PointerEvent<HTMLSpanElement>) => void;
  onResizePointerDown: (event: PointerEvent<HTMLSpanElement>) => void;
  onResizePointerMove: (event: PointerEvent<HTMLSpanElement>) => void;
  onResizePointerUp: (event: PointerEvent<HTMLSpanElement>) => void;
};

export function WorkflowNode({
  node,
  definition,
  workflowNode,
  diffState,
  dirty,
  locked,
  connectionDisabled,
  portHighlights,
  selected,
  stepState,
  stepError,
  referenceBagLabel,
  artifactOutputs,
  resizable,
  embedded = false,
  workspaceId,
  onSelectOutput,
  onKeyboardSelect,
  onOutputPortPointerDown,
  onPointerCancel,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onResizePointerCancel,
  onResizePointerDown,
  onResizePointerMove,
  onResizePointerUp,
}: WorkflowNodeProps) {
  const params = paramsFromSummary(node.summary);
  const active = stepState === 'running';
  const done = stepState === 'succeeded';
  const failed = stepState === 'failed';
  const cached = done && node.cached;
  const inputs = definition?.inputs ?? [];
  const outputs = definition?.outputs ?? [];
  const primaryTextParam = selected ? primaryNodeTextParam(workflowNode?.params) : null;
  const media = isCanvasCardType(node.nodeType);
  const title = mediaCardTitle(node);
  const textValue = stringNodeParam(workflowNode?.params, 'text');
  const classes = [
    'node',
    diffState === 'add' ? 'node--add' : '',
    diffState === 'upd' ? 'node--upd' : '',
    dirty ? 'node--dirty' : '',
    locked ? 'node--locked' : '',
    embedded ? 'node--embedded' : '',
    media ? 'node--media' : '',
    selected ? 'p-sel' : '',
    active ? 'p-active' : '',
    failed ? 'node--err' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div
      aria-label={`${title} (${node.nodeType})`}
      aria-current={selected ? 'true' : undefined}
      className={classes}
      onKeyDown={(event: KeyboardEvent<HTMLDivElement>) => {
        if (event.key !== 'Enter' && event.key !== ' ') return;
        event.preventDefault();
        event.stopPropagation();
        onKeyboardSelect?.(event.shiftKey || event.metaKey || event.ctrlKey);
      }}
      onClick={embedded ? undefined : (event) => event.stopPropagation()}
      onPointerCancel={onPointerCancel}
      onPointerDown={(event) => {
        if (isNodeInteractiveTarget(event.target)) {
          event.stopPropagation();
          return;
        }
        onPointerDown(event);
      }}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      role="group"
      style={{
        left: embedded ? undefined : node.position.x,
        top: embedded ? undefined : node.position.y,
        width: embedded ? undefined : graphNodeWidth(node),
        height: embedded ? undefined : graphNodeHeight(node),
        minHeight: embedded ? undefined : graphNodeHeight(node),
        '--swatch': categorySwatch(node.category),
      } as CSSProperties}
      tabIndex={embedded ? -1 : 0}
    >
      {done && (
        <span className="p-done">
          <Icon n="check" s={10} sw={2.2} />
        </span>
      )}
      {active && <span className="p-spin" />}
      {diffState === 'add' && <span className="node-flag add">+ 新增</span>}
      {diffState === 'upd' && <span className="node-flag upd">~ 修改</span>}
      {cached && <span className="node-flag cache">缓存</span>}
      {failed && <span className="node-flag err">失败</span>}
      {selected && !media && (
        <span className="node-flag selected">SELECTED</span>
      )}
      {media && <div className="node-head">{title}</div>}
      {!media && (
        <div className="node-title">
          <span className="swatch" />
          {title}
          <span className="p-nid">{node.id}</span>
        </div>
      )}
      <div className="node-body">
        {!media && (
        <div className="io-row">
          <span className="port-stack port-stack--input">
            {inputs.length === 0 ? (
              <span className="port-empty">{node.category}</span>
            ) : (
              inputs.map((port, index) => (
                <span
                  className={portTargetClass(
                    'input',
                    portHighlights.get(portKey(node.id, 'input', port.name)),
                    connectionDisabled,
                  )}
                  data-port-direction="input"
                  data-port-index={index}
                  data-port-name={port.name}
                  data-port-node-id={node.id}
                  data-port-type={port.type}
                  key={port.name}
                  title={`${port.name} · ${port.type}`}
                >
                  <Port type={port.type} />
                  <span>{port.name}</span>
                </span>
              ))
            )}
          </span>
          <span className="port-stack port-stack--output">
            {outputs.length === 0 ? (
              <span className="port-empty">{node.nodeType.split('.').at(-1) ?? 'out'}</span>
            ) : (
              outputs.map((port, index) => (
                <span
                  className={portTargetClass(
                    'output',
                    portHighlights.get(portKey(node.id, 'output', port.name)),
                    connectionDisabled,
                  )}
                  data-port-direction="output"
                  data-port-index={index}
                  data-port-name={port.name}
                  data-port-node-id={node.id}
                  data-port-type={port.type}
                  key={port.name}
                  onPointerDown={(event) => {
                    if (!connectionDisabled) {
                      onOutputPortPointerDown(node, port, index, event);
                    }
                  }}
                  title={`${port.name} · ${port.type}`}
                >
                  <span>{port.name}</span>
                  <Port type={port.type} />
                </span>
              ))
            )}
          </span>
        </div>
        )}
        {referenceBagLabel && <div className="node-refbag">{referenceBagLabel}</div>}
        {stepError && <div className="node-step-error">{stepError}</div>}
        {node.nodeType === 'input.text' ? (
          <div className={textValue ? 'text-card' : 'text-card text-card--empty'}>
            <div className="text-card-kicker">Text</div>
            {textValue ? (
              <div
                className="text-card-body"
                dangerouslySetInnerHTML={{ __html: markdownToHtml(textValue) }}
              />
            ) : null}
          </div>
        ) : node.nodeType === 'input.video' ? (
          <VideoMediaCard
            artifacts={artifactOutputs}
            nodeId={node.id}
            selected={selected}
            storageUri={
              workflowNode?.params && typeof workflowNode.params === 'object'
                ? (workflowNode.params as Record<string, unknown>).storage_uri
                : undefined
            }
            workspaceId={workspaceId}
          />
        ) : media ? (
          <MediaCard
            artifacts={artifactOutputs}
            nodeType={node.nodeType}
            storageUri={
              workflowNode?.params && typeof workflowNode.params === 'object'
                ? (workflowNode.params as Record<string, unknown>).storage_uri
                : undefined
            }
            workspaceId={workspaceId}
          />
        ) : (
          <>
        {primaryTextParam && (
          <div className="node-inline-editor">
            <span className="node-inline-label">{primaryTextParam.key}</span>
            <pre>{primaryTextParam.value}</pre>
          </div>
        )}
        {params.length === 0 ? (
          <div className="param-row">
            <span className="param-k">type</span>
            <span className="param-v">{node.nodeType}</span>
          </div>
        ) : (
          params.slice(0, 4).map((param) => (
            <div className="param-row" key={param.key}>
              <span className="param-k">{param.key}</span>
              <span className="param-v">{param.value}</span>
            </div>
          ))
        )}
          </>
        )}
        {artifactOutputs.length > 0 && !media && (
          <div className="node-artifacts">
            {artifactOutputs.slice(0, 1).map((output) => (
              <button
                className={output.selected ? 'node-artifact node-artifact--selected' : 'node-artifact'}
                key={output.id}
                onClick={(event) => {
                  event.stopPropagation();
                  onSelectOutput?.(output.id);
                }}
                title={output.meta || output.title}
                type="button"
              >
                {output.preview?.kind === 'image' ? (
                  <img alt="" className="node-artifact-preview" src={output.preview.content} />
                ) : (
                  <span className="node-artifact-icon"><Icon n={artifactIcon(output.kind)} s={12} /></span>
                )}
                <span className="node-artifact-copy">
                  <strong>{output.title}</strong>
                  <small>{output.kind}</small>
                </span>
              </button>
            ))}
            {artifactOutputs.length > 1 && (
              <span className="node-artifact-more">+{artifactOutputs.length - 1} 个结果</span>
            )}
          </div>
        )}
      </div>
      {resizable && (
        <span
          aria-label={`Resize ${node.title}`}
          className="node-resize-handle"
          onPointerCancel={onResizePointerCancel}
          onPointerDown={onResizePointerDown}
          onPointerMove={onResizePointerMove}
          onPointerUp={onResizePointerUp}
          title="Resize node"
        />
      )}
    </div>
  );
}

function primaryNodeTextParam(params: unknown): { key: string; value: string } | null {
  if (!params || typeof params !== 'object' || Array.isArray(params)) return null;
  const entries = Object.entries(params as Record<string, unknown>);
  const preferred = ['brief', 'prompt', 'caption', 'text', 'style', 'directive'];
  const found = preferred
    .map((key) => entries.find(([entryKey]) => entryKey.toLowerCase() === key))
    .find((entry): entry is [string, unknown] => Boolean(entry));
  const fallback = found ?? entries.find(([, value]) => typeof value === 'string');
  if (!fallback) return null;
  const [key, value] = fallback;
  if (typeof value !== 'string' && typeof value !== 'number') return null;
  const text = String(value).trim();
  return text.length > 0 ? { key, value: text } : null;
}

function isNodeInteractiveTarget(target: EventTarget | null): boolean {
  const candidate = target as (EventTarget & { closest?: (selector: string) => Element | null }) | null;
  if (!candidate) return false;
  return typeof candidate.closest === 'function' &&
    Boolean(candidate.closest('button, input, textarea, select, a'));
}

function artifactIcon(kind: string): 'export' | 'image' | 'layers' | 'play' {
  if (kind === 'image') return 'image';
  if (kind === 'video') return 'play';
  if (kind === 'html' || kind === 'markdown' || kind === 'text' || kind === 'json') {
    return 'export';
  }
  return 'layers';
}

function portTargetClass(
  direction: 'input' | 'output',
  highlight: PortHighlight | undefined,
  disabled: boolean,
): string {
  return [
    'port-target',
    `port-target--${direction}`,
    disabled ? 'port-target--disabled' : '',
    highlight ? `port-target--${highlight}` : '',
  ]
    .filter(Boolean)
    .join(' ');
}

function stringNodeParam(params: unknown, key: string): string {
  if (!params || typeof params !== 'object' || Array.isArray(params)) return '';
  const value = (params as Record<string, unknown>)[key];
  return typeof value === 'string' ? value : '';
}

function mediaCardTitle(node: GraphNodeState): string {
  if (!isMediaNodeType(node.nodeType)) return node.title;
  const kind = mediaKindLabel(node.nodeType);
  const generic = new Set([
    kind,
    'Image Input',
    'image.input',
    'Video Input',
    'video.input',
    'Audio Input',
    'audio.input',
  ]);
  return generic.has(node.title) ? kind : node.title;
}

function MediaCard({
  artifacts,
  nodeType,
  storageUri,
  workspaceId,
}: {
  artifacts: CanvasNodeArtifact[];
  nodeType: string;
  storageUri: unknown;
  workspaceId?: string;
}) {
  const url = useMediaPreviewUrl(workspaceId, storageUri, artifacts, nodeType);
  if (!url) {
    const icon = nodeType === 'input.audio' ? 'music' : 'image';
    return (
      <div className="media-card media-card--empty">
        <span aria-hidden="true" className="media-card-icon">
          <Icon n={icon} s={32} sw={1.4} />
        </span>
        <span className="media-card-kind">{mediaKindLabel(nodeType)}</span>
      </div>
    );
  }
  if (nodeType === 'input.audio') {
    return (
      <div className="media-card media-card--audio">
        <audio controls preload="metadata" src={url} />
      </div>
    );
  }
  return <img alt="" className="media-card-frame" decoding="async" draggable={false} loading="lazy" src={url} />;
}

function VideoMediaCard({
  artifacts,
  nodeId,
  selected,
  storageUri,
  workspaceId,
}: {
  artifacts: CanvasNodeArtifact[];
  nodeId: string;
  selected: boolean;
  storageUri: unknown;
  workspaceId?: string;
}) {
  const [hover, setHover] = useState(false);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const poster = videoPosterUrl(artifacts);
  const hasSource = videoHasSource(artifacts, storageUri);
  const priority = hover
    ? CANVAS_VIDEO_DECODER_PRIORITY.hover
    : selected
      ? CANVAS_VIDEO_DECODER_PRIORITY.selected
      : CANVAS_VIDEO_DECODER_PRIORITY.visible;
  const decode = useCanvasVideoDecoder(nodeId, priority, hasSource);
  const url = useMediaPreviewUrl(workspaceId, storageUri, artifacts, 'input.video', decode);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (hover) {
      void video.play().catch(() => undefined);
      return;
    }
    video.pause();
  }, [hover, url]);

  if (!hasSource) {
    return (
      <div className="media-card media-card--empty">
        <span aria-hidden="true" className="media-card-icon">
          <Icon n="play" s={32} sw={1.4} />
        </span>
        <span className="media-card-kind">{mediaKindLabel('input.video')}</span>
      </div>
    );
  }

  return (
    <div
      className="media-card-video"
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
    >
      {decode && url ? (
        <video
          className="media-card-frame"
          controls={hover}
          data-canvas-video="live"
          muted
          playsInline
          poster={poster ?? undefined}
          preload="metadata"
          ref={videoRef}
          src={url}
        />
      ) : (
        <div className="media-card-frame media-card-frame--poster" data-canvas-video="poster">
          {poster ? (
            <img alt="" decoding="async" draggable={false} src={poster} />
          ) : (
            <span aria-hidden="true" className="media-card-poster-play" />
          )}
        </div>
      )}
    </div>
  );
}

function videoHasSource(artifacts: CanvasNodeArtifact[], storageUri: unknown): boolean {
  if (parseUploadUri(storageUri)) return true;
  if (videoPosterUrl(artifacts)) return true;
  return artifacts.some((item) => item.preview?.kind === 'video' || item.kind === 'video');
}

function videoPosterUrl(artifacts: CanvasNodeArtifact[]): string | null {
  const image = artifacts.find((item) => item.preview?.kind === 'image');
  return image?.preview && 'content' in image.preview ? image.preview.content : null;
}

function useMediaPreviewUrl(
  workspaceId: string | undefined,
  storageUri: unknown,
  artifacts: CanvasNodeArtifact[],
  nodeType: string,
  enabled = true,
): string | null {
  const artifactPreview = artifacts.find((item) => {
    if (nodeType === 'input.video') return item.preview?.kind === 'video' || item.kind === 'video';
    if (nodeType === 'input.audio') return item.kind === 'audio';
    return item.preview?.kind === 'image' || item.kind === 'image';
  });
  const immediate =
    artifactPreview?.preview && 'content' in artifactPreview.preview
      ? artifactPreview.preview.content
      : null;
  const uploadId = parseUploadUri(storageUri);
  const [uploadUrl, setUploadUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled || !workspaceId || !uploadId || immediate) {
      setUploadUrl(null);
      return;
    }
    let cancelled = false;
    let objectUrl: string | null = null;
    void fetchWorkspaceUploadContent(workspaceId, uploadId)
      .then((blob) => {
        objectUrl = URL.createObjectURL(blob);
        if (!cancelled) setUploadUrl(objectUrl);
      })
      .catch(() => {
        if (!cancelled) setUploadUrl(null);
      });
    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [enabled, immediate, uploadId, workspaceId]);

  if (!enabled) return null;
  return uploadUrl ?? immediate;
}
