import { useEffect, useRef, useState } from 'react';
import { setPendingPrompt, useCreationStore } from '../../creation-store';
import { isVisualMediaCardType } from '../../grid-split';
import { Icon } from '../../icons';
import { ModelPicker } from '../model-picker';
import { useWorkbenchStore } from '../../store';
import type { ModelCatalog, WorkbenchState } from '../../types';
import { mediaGenerateTarget } from '../graph-canvas-editing';
import type { ViewState } from '../graph-canvas-navigation';
import { MentionTextarea, type MentionItem } from '../mention-textarea';
import { flowAboveCenterStyle, flowBelowStyle } from './overlay-anchor';

type MediaNode = WorkbenchState['graph']['nodes'][number];

export function EmptyCardUpload({
  node,
  view: _view,
  onUpload,
}: {
  node: MediaNode;
  view: ViewState;
  onUpload: (file: File) => void;
}) {
  const inputRef = useRef<HTMLInputElement | null>(null);
  const accept = node.nodeType === 'input.video'
    ? 'video/*'
    : node.nodeType === 'input.audio'
      ? 'audio/*'
      : 'image/*';
  return (
    <div
      className="canvas-media-upload nodrag nopan"
      style={flowAboveCenterStyle(node, 14)}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <button onClick={() => inputRef.current?.click()} type="button">
        <Icon n="export" s={12} sw={1.8} />
        上传
      </button>
      <input
        accept={accept}
        hidden
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          event.currentTarget.value = '';
          if (file) onUpload(file);
        }}
        ref={inputRef}
        type="file"
      />
    </div>
  );
}

export function mentionItemsFromNodes(
  nodes: WorkbenchState['graph']['nodes'],
  currentId: string,
): MentionItem[] {
  return nodes
    .filter((node) => node.id !== currentId && isVisualMediaCardType(node.nodeType))
    .map((node) => ({
      id: node.id,
      label: (node.title || node.id).replace(/\s+/g, ''),
      title: node.title || node.id,
      kind: node.nodeType === 'input.video' ? '视频' : node.nodeType === 'input.audio' ? '音频' : '图片',
    }));
}

export function MediaCardComposer({
  hasSource,
  mentionItems,
  modelCatalog,
  node,
  pickingReference,
  providers,
  view: _view,
  workflowPrompt,
  onMention,
  onPickReference,
}: {
  hasSource?: boolean;
  mentionItems: MentionItem[];
  modelCatalog: ModelCatalog | null;
  node: MediaNode;
  pickingReference?: boolean;
  providers?: WorkbenchState['providers'];
  view: ViewState;
  workflowPrompt: string;
  onMention?: (item: MentionItem) => void;
  onPickReference?: () => void;
}) {
  const generateFromMediaCard = useWorkbenchStore((state) => state.generateFromMediaCard);
  const { pendingPrompt } = useCreationStore();
  const textCard = node.nodeType === 'input.text';
  const [kind, setKind] = useState<'image' | 'video'>(
    node.nodeType === 'input.video' || node.nodeType.startsWith('video.') ? 'video' : 'image',
  );
  const [prompt, setPrompt] = useState(workflowPrompt);
  const [aspect, setAspect] = useState('1:1');
  const [durationSec, setDurationSec] = useState(5);
  const [count, setCount] = useState(1);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const target = mediaGenerateTarget({
    nodeType: node.nodeType,
    hasImage: Boolean(hasSource) && !node.nodeType.startsWith('video.') && node.nodeType !== 'input.video',
    mediaKind: textCard ? kind : undefined,
  });
  useEffect(() => {
    setPrompt(workflowPrompt);
  }, [node.id, workflowPrompt]);
  useEffect(() => {
    if (!pendingPrompt) return;
    setPrompt(pendingPrompt);
    setPendingPrompt(null);
  }, [pendingPrompt]);
  if (!target) return null;
  const video = target.mediaKind === 'video';
  return (
    <form
      className="media-card-composer nodrag nopan"
      onPointerDown={(event) => event.stopPropagation()}
      onSubmit={(event) => {
        event.preventDefault();
        setBusy(true);
        setError(null);
        void generateFromMediaCard(node.id, prompt, aspect, {
          durationSec: video ? durationSec : undefined,
          count,
          mediaKind: textCard ? kind : undefined,
        })
          .catch((caught) => {
            setError(caught instanceof Error ? caught.message : '生成失败');
          })
          .finally(() => setBusy(false));
      }}
      style={flowBelowStyle(node, 14)}
    >
      <MentionTextarea
        items={mentionItems}
        onChange={setPrompt}
        onMention={onMention}
        placeholder={
          video
            ? '描述你想生成的视频'
            : textCard
              ? '描述你想生成的画面'
              : '描述你想生成的画面，输入 @ 引用画布上的图'
        }
        rows={2}
        value={prompt}
      />
      <div className="media-card-composer-foot">
        {textCard ? (
          <label className="media-card-composer-chip media-card-composer-chip--select">
            <select
              aria-label="生成类型"
              onChange={(event) => setKind(event.currentTarget.value as 'image' | 'video')}
              value={kind}
            >
              <option value="image">出图</option>
              <option value="video">出视频</option>
            </select>
          </label>
        ) : null}
        <ModelPicker
          capabilityId={target.capabilityId}
          catalog={modelCatalog}
          disabled={busy}
          providers={providers}
        />
        <label className="media-card-composer-chip media-card-composer-chip--select">
          <select
            aria-label="画面比例"
            onChange={(event) => setAspect(event.currentTarget.value)}
            value={aspect}
          >
            <option value="1:1">1:1</option>
            <option value="16:9">16:9</option>
            <option value="9:16">9:16</option>
          </select>
        </label>
        {video ? (
          <label className="media-card-composer-chip media-card-composer-chip--select">
            <select
              aria-label="时长"
              onChange={(event) => setDurationSec(Number(event.currentTarget.value))}
              value={String(durationSec)}
            >
              <option value="4">4s</option>
              <option value="5">5s</option>
              <option value="8">8s</option>
              <option value="10">10s</option>
            </select>
          </label>
        ) : null}
        <label className="media-card-composer-chip media-card-composer-chip--select">
          <select
            aria-label="张数"
            onChange={(event) => setCount(Number(event.currentTarget.value))}
            value={String(count)}
          >
            <option value="1">x1</option>
            <option value="2">x2</option>
            <option value="4">x4</option>
          </select>
        </label>
        {video || textCard ? null : (
          <button
            className={`media-card-composer-chip${pickingReference ? ' is-active' : ''}`}
            onClick={onPickReference}
            type="button"
          >
            {pickingReference ? '点一张图当参考' : '选参考'}
          </button>
        )}
        <button
          aria-label={busy ? '生成中' : '生成'}
          className="media-card-composer-send"
          disabled={busy || prompt.trim().length === 0}
          type="submit"
        >
          <Icon n="arrowUp" s={14} sw={2} />
        </button>
      </div>
      {error ? <div className="inspector-error">{error}</div> : null}
    </form>
  );
}
