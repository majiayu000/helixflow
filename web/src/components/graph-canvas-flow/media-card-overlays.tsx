import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { setPendingPrompt, useCreationStore } from '../../creation-store';
import { isVisualMediaCardType } from '../../grid-split';
import { Icon } from '../../icons';
import { ModelPicker } from '../model-picker';
import { useWorkbenchStore } from '../../store';
import type { ModelCatalog, WorkbenchState } from '../../types';
import { graphNodeHeight, graphNodeWidth, type ViewState } from '../graph-canvas-navigation';
import { MentionTextarea, type MentionItem } from '../mention-textarea';

type MediaNode = WorkbenchState['graph']['nodes'][number];

export function EmptyCardUpload({
  node,
  view,
  onUpload,
}: {
  node: MediaNode;
  view: ViewState;
  onUpload: (file: File) => void;
}) {
  const inputRef = useRef<HTMLInputElement | null>(null);
  const left = node.position.x * view.z + view.x + (graphNodeWidth(node) * view.z) / 2;
  const top = node.position.y * view.z + view.y - 14;
  const accept = node.nodeType === 'input.video'
    ? 'video/*'
    : node.nodeType === 'input.audio'
      ? 'audio/*'
      : 'image/*';
  return (
    <div
      className="canvas-media-upload"
      style={{ left, top } as CSSProperties}
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
  mentionItems,
  modelCatalog,
  node,
  pickingReference,
  providers,
  view,
  workflowPrompt,
  onMention,
  onPickReference,
}: {
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
  const [prompt, setPrompt] = useState(workflowPrompt);
  const [aspect, setAspect] = useState('1:1');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setPrompt(workflowPrompt);
  }, [node.id, workflowPrompt]);
  useEffect(() => {
    if (!pendingPrompt) return;
    setPrompt(pendingPrompt);
    setPendingPrompt(null);
  }, [pendingPrompt]);
  const cardWidth = graphNodeWidth(node) * view.z;
  const width = Math.max(560, cardWidth);
  const left = node.position.x * view.z + view.x + (cardWidth - width) / 2;
  const top = (node.position.y + graphNodeHeight(node)) * view.z + view.y + 14;
  return (
    <form
      className="media-card-composer"
      onPointerDown={(event) => event.stopPropagation()}
      onSubmit={(event) => {
        event.preventDefault();
        setBusy(true);
        setError(null);
        void generateFromMediaCard(node.id, prompt, aspect)
          .catch((caught) => {
            setError(caught instanceof Error ? caught.message : '生成失败');
          })
          .finally(() => setBusy(false));
      }}
      style={{ left, top, width } as CSSProperties}
    >
      <MentionTextarea
        items={mentionItems}
        onChange={setPrompt}
        onMention={onMention}
        placeholder="描述你想生成的画面，输入 @ 引用画布上的图"
        rows={2}
        value={prompt}
      />
      <div className="media-card-composer-foot">
        <ModelPicker
          capabilityId="text_to_image"
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
        <button
          className={`media-card-composer-chip${pickingReference ? ' is-active' : ''}`}
          onClick={onPickReference}
          type="button"
        >
          {pickingReference ? '点一张图当参考' : '选参考'}
        </button>
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
