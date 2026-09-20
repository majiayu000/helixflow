import { useEffect, useMemo, useState } from 'react';
import {
  listCreationAssets,
  readCreationAssetFile,
  type CreationAsset,
} from '../../creation-assets';
import { setPendingPrompt } from '../../creation-store';
import { parseUploadUri } from '../../grid-split';
import { loadPromptLibrary, searchPromptLibrary, type PromptEntry } from '../../prompt-library';
import type { NodeDefinition, WorkbenchState } from '../../types';

export type BrowseTab = 'library' | 'history' | 'search' | 'templates' | 'prompts';

const TEMPLATES = [
  { id: 'ad', name: '产品广告链路', desc: '文案 + 产品图 → 视频' },
  { id: 'char', name: '人物设定', desc: '三张图片并排' },
] as const;

export function CanvasBrowsePanel({
  nodes,
  outputs,
  tab,
  workflowGraph,
  onClose,
  onDuplicate,
  onJump,
  onPlaceFile,
  onTemplate,
}: {
  nodes: WorkbenchState['graph']['nodes'];
  outputs: WorkbenchState['outputs'] | undefined;
  tab: BrowseTab;
  workflowGraph?: WorkbenchState['workflowGraph'];
  onClose: () => void;
  onDuplicate: (node: WorkbenchState['graph']['nodes'][number]) => void;
  onJump: (nodeId: string) => void;
  onPlaceFile?: (file: File) => void;
  onTemplate: (id: 'ad' | 'char') => void;
}) {
  const [query, setQuery] = useState('');
  const [assets, setAssets] = useState<CreationAsset[]>([]);
  const [prompts, setPrompts] = useState<PromptEntry[]>([]);
  const [promptError, setPromptError] = useState<string | null>(null);
  const q = query.trim().toLowerCase();
  const library = useMemo(
    () => nodes.filter((node) => {
      const params = workflowGraph?.nodes[node.id]?.params;
      return Boolean(parseUploadUri(
        params && typeof params === 'object' && !Array.isArray(params)
          ? (params as Record<string, unknown>).storage_uri
          : undefined,
      ));
    }),
    [nodes, workflowGraph],
  );
  const history = outputs ?? [];
  const searchHits = useMemo(
    () => nodes.filter((node) => {
      if (!q) return true;
      const params = workflowGraph?.nodes[node.id]?.params;
      const prompt = params && typeof params === 'object' && !Array.isArray(params)
        ? String((params as Record<string, unknown>).prompt ?? (params as Record<string, unknown>).text ?? '')
        : '';
      return `${node.title} ${node.nodeType} ${prompt}`.toLowerCase().includes(q);
    }),
    [nodes, q, workflowGraph],
  );
  const visibleAssets = assets.filter((asset) => !q || asset.title.toLowerCase().includes(q));
  const visiblePrompts = useMemo(() => searchPromptLibrary(prompts, query), [prompts, query]);

  useEffect(() => {
    if (tab !== 'library') return;
    void listCreationAssets().then(setAssets).catch(() => setAssets([]));
  }, [tab]);

  useEffect(() => {
    if (tab !== 'prompts') return;
    setPromptError(null);
    void loadPromptLibrary()
      .then(setPrompts)
      .catch((error) => {
        setPromptError(error instanceof Error ? error.message : '提示词库加载失败');
      });
  }, [tab]);

  return (
    <aside className="canvas-browse" onPointerDown={(event) => event.stopPropagation()}>
      <div className="canvas-browse-hd">
        <strong>{tabLabel(tab)}</strong>
        <button onClick={onClose} type="button">关闭</button>
      </div>
      {(tab === 'library' || tab === 'search' || tab === 'prompts') && (
        <input
          onChange={(event) => setQuery(event.currentTarget.value)}
          autoFocus={tab === 'search'}
          placeholder={
            tab === 'search' ? '搜索标题 / Prompt' : tab === 'prompts' ? '搜索提示词' : '搜索素材'
          }
          value={query}
        />
      )}
      <div className="canvas-browse-list">
        {tab === 'library' && (
          <>
            {visibleAssets.map((asset) => (
              <button
                key={asset.id}
                onClick={() => {
                  void readCreationAssetFile(asset.id).then((file) => onPlaceFile?.(file));
                }}
                type="button"
              >
                <div>{asset.title}<small>素材库 · {asset.kind}</small></div>
              </button>
            ))}
            {library.length === 0 && visibleAssets.length === 0 ? (
              <div className="canvas-browse-empty">还没有素材。工具条「存素材」会写进这里。</div>
            ) : library.filter((node) => !q || node.title.toLowerCase().includes(q)).map((node) => (
              <button key={node.id} onClick={() => onDuplicate(node)} type="button">
                <div>{node.title}<small>当前画布副本</small></div>
              </button>
            ))}
          </>
        )}
        {tab === 'history' && (history.length === 0 ? (
          <div className="canvas-browse-empty">还没有生成记录</div>
        ) : history.map((output) => (
          <button
            key={output.id}
            onClick={() => output.nodeId && onJump(output.nodeId)}
            type="button"
          >
            <div>{output.title || output.id}<small>{output.kind}</small></div>
          </button>
        )))}
        {tab === 'search' && (searchHits.length === 0 ? (
          <div className="canvas-browse-empty">没有匹配的节点</div>
        ) : searchHits.map((node) => (
          <button key={node.id} onClick={() => onJump(node.id)} type="button">
            <div>{node.title}<small>{node.nodeType}</small></div>
          </button>
        )))}
        {tab === 'prompts' && (
          <>
            {promptError ? <div className="canvas-browse-empty">{promptError}</div> : null}
            {!promptError && visiblePrompts.length === 0 ? (
              <div className="canvas-browse-empty">没有匹配的提示词</div>
            ) : visiblePrompts.map((item) => (
              <button
                key={item.id}
                onClick={() => {
                  setPendingPrompt(item.prompt);
                  onClose();
                }}
                type="button"
              >
                <div>{item.title}<small>{item.sourceName}</small></div>
              </button>
            ))}
          </>
        )}
        {tab === 'templates' && TEMPLATES.map((item) => (
          <button key={item.id} onClick={() => onTemplate(item.id)} type="button">
            <div>{item.name}<small>{item.desc}</small></div>
          </button>
        ))}
      </div>
    </aside>
  );
}

export function buildTemplateOps(
  id: 'ad' | 'char',
  definitions: Map<string, NodeDefinition>,
  origin: { x: number; y: number },
): { definition: NodeDefinition; position: { x: number; y: number } }[] {
  if (id === 'char') {
    const image = definitions.get('input.image');
    if (!image) return [];
    return [0, 1, 2].map((index) => ({
      definition: image,
      position: { x: origin.x + index * 360, y: origin.y },
    }));
  }
  const text = definitions.get('input.text');
  const image = definitions.get('input.image');
  const video = definitions.get('input.video');
  if (!text || !image || !video) return [];
  return [
    { definition: text, position: origin },
    { definition: image, position: { x: origin.x + 360, y: origin.y } },
    { definition: video, position: { x: origin.x + 720, y: origin.y } },
  ];
}

function tabLabel(tab: BrowseTab): string {
  if (tab === 'library') return '素材库';
  if (tab === 'history') return '历史';
  if (tab === 'search') return '节点搜索';
  if (tab === 'prompts') return '提示词库';
  return '模板';
}
