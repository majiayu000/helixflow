import { useMemo, useRef, useState } from 'react';
import { Icon, Port, type IconName } from '../icons';
import type { ModelCatalog, NodeCatalog, NodeDefinition, WorkbenchState } from '../types';
import type { BrowseTab } from './graph-canvas-flow/canvas-browse-panel';
import { ModelCatalogTray } from './model-catalog-tray';

type NodeLibraryProps = {
  catalog: NodeCatalog | null;
  modelCatalog: ModelCatalog | null;
  modelCatalogError: string | null;
  availableBindingIds?: string[];
  error: string | null;
  disabled: boolean;
  providers?: WorkbenchState['providers'];
  onAddNode: (definition: NodeDefinition) => void;
  onOpenBrowse?: (tab: BrowseTab) => void;
  onOpenComments?: () => void;
  onRedo?: () => void;
  onUndo?: () => void;
  onUploadFiles?: (files: File[]) => void;
};

export function NodeLibrary({
  catalog,
  modelCatalog,
  modelCatalogError,
  availableBindingIds,
  error,
  disabled,
  providers,
  onAddNode,
  onOpenBrowse,
  onOpenComments,
  onRedo,
  onUndo,
  onUploadFiles,
}: NodeLibraryProps) {
  const uploadRef = useRef<HTMLInputElement | null>(null);
  const [query, setQuery] = useState('');
  const [category, setCategory] = useState('select');
  const [open, setOpen] = useState(false);
  const definitions = catalog?.nodes ?? [];
  const tools = useMemo(
    () => toolbarItems(definitions),
    [definitions],
  );
  const activeCategory = category === 'select' ? 'all' : category;
  const filtered = useMemo(
    () => filterNodeDefinitions(definitions, query, activeCategory),
    [activeCategory, definitions, query],
  );
  const showTray = open && category !== 'select';
  const visibleItems = query.trim() ? filtered : filtered.slice(0, 8);
  const definitionsByCategory = useMemo(() => groupDefinitionsByCategory(definitions), [definitions]);

  const selectTool = (tool: ToolbarItem) => {
    if (tool.category === 'upload') {
      uploadRef.current?.click();
      return;
    }
    if (tool.category === 'undo') {
      onUndo?.();
      return;
    }
    if (tool.category === 'redo') {
      onRedo?.();
      return;
    }
    if (tool.category === 'comments') {
      onOpenComments?.();
      setCategory('select');
      setOpen(false);
      return;
    }
    if (
      tool.category === 'output'
      || tool.category === 'search'
      || tool.category === 'history'
      || tool.category === 'templates'
      || tool.category === 'prompts'
    ) {
      onOpenBrowse?.(tool.category === 'output' ? 'library' : tool.category);
      setCategory('select');
      setOpen(false);
      return;
    }
    if (tool.category === 'select') {
      setCategory('select');
      setOpen(false);
      return;
    }

    if (open && category === tool.category) {
      setCategory('select');
      setOpen(false);
      return;
    }

    const mediaCard = mediaCardDefinition(definitions, tool.category);
    if (mediaCard) {
      if (!disabled) onAddNode(mediaCard);
      setCategory('select');
      setOpen(false);
      return;
    }

    setQuery('');
    setCategory(tool.category);

    const categoryDefinitions =
      tool.category === 'all' ? definitions : definitionsByCategory.get(tool.category) ?? [];
    if (
      !disabled &&
      tool.category !== 'all' &&
      tool.category !== 'models' &&
      categoryDefinitions.length === 1
    ) {
      setOpen(false);
      onAddNode(categoryDefinitions[0]!);
      return;
    }

    setOpen(true);
  };

  const addFromTray = (definition: NodeDefinition) => {
    if (disabled) return;
    onAddNode(definition);
    setCategory('select');
    setOpen(false);
  };

  return (
    <aside className="node-library" onPointerDown={(event) => event.stopPropagation()}>
      <div className="node-library-bar" role="toolbar" aria-label="节点工具条">
        {tools.map((tool) => (
          <FragmentedToolButton
            active={tool.category === category}
            disabled={disabled || Boolean(tool.disabled)}
            key={tool.key}
            leadingDivider={Boolean(tool.dividerBefore)}
            onClick={() => selectTool(tool)}
            tool={tool}
          />
        ))}
        <input
          accept="image/*,video/*,audio/*"
          hidden
          multiple
          onChange={(event) => {
            const files = [...(event.currentTarget.files ?? [])];
            event.currentTarget.value = '';
            if (files.length && onUploadFiles) onUploadFiles(files);
          }}
          ref={uploadRef}
          type="file"
        />
      </div>
      {showTray && (
        <div className="node-library-tray">
          <div className="node-library-tray-head">
            <strong>{toolLabel(category)}</strong>
            {disabled && <span>待处理变更</span>}
          </div>
          {category === 'models' && (
            <ModelCatalogTray
              disabled={disabled}
              error={modelCatalogError}
              modelCatalog={modelCatalog}
              nodeCatalog={catalog}
              onAddNode={onAddNode}
              providers={providers}
            />
          )}
          {category !== 'models' && (
            <input
              aria-label="搜索节点"
              className="node-library-search"
              disabled={disabled}
              placeholder="搜索节点"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          )}
          {category !== 'models' && error && <div className="node-library-empty">{error}</div>}
          {category !== 'models' && !error && definitions.length === 0 && (
            <div className="node-library-empty">暂无节点</div>
          )}
          {category !== 'models' && !error && definitions.length > 0 && filtered.length === 0 && (
            <div className="node-library-empty">无匹配节点</div>
          )}
          {category !== 'models' && !error && filtered.length > 0 && (
            <div className="node-library-list">
              {visibleItems.map((definition) => (
                <button
                  className="node-library-item"
                  disabled={disabled}
                  draggable={!disabled}
                  key={definition.type}
                  onClick={() => addFromTray(definition)}
                  onDragStart={(event) => {
                    event.dataTransfer.effectAllowed = 'copy';
                    event.dataTransfer.setData('application/x-helixflow-node-type', definition.type);
                  }}
                  type="button"
                >
                  <span className="node-library-title">
                    <Port type={definition.category} />
                    {contentCardLabel(definition.type)}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </aside>
  );
}

type ToolbarItem = {
  category: string;
  dividerBefore?: boolean;
  disabled?: boolean;
  icon: IconName;
  key: string;
  label: string;
  primary?: boolean;
  text?: string;
};

function FragmentedToolButton({
  active,
  disabled,
  leadingDivider,
  onClick,
  tool,
}: {
  active: boolean;
  disabled: boolean;
  leadingDivider: boolean;
  onClick: () => void;
  tool: ToolbarItem;
}) {
  return (
    <>
      {leadingDivider && <span className="node-library-divider" />}
      <button
        aria-pressed={active}
        className={toolClassName(active, tool)}
        disabled={disabled}
        onClick={onClick}
        title={tool.label}
        type="button"
      >
        <Icon n={tool.icon} s={18} sw={1.9} />
        {tool.text && <span>{tool.text}</span>}
      </button>
    </>
  );
}

const MEDIA_CARD_TYPE: Record<string, string> = {
  text: 'input.text',
  image: 'input.image',
  video: 'input.video',
  audio: 'input.audio',
};

function mediaCardDefinition(definitions: NodeDefinition[], category: string): NodeDefinition | undefined {
  const type = MEDIA_CARD_TYPE[category];
  return type ? definitions.find((item) => item.type === type) : undefined;
}

function toolbarItems(
  definitions: NodeDefinition[],
): ToolbarItem[] {
  const available = new Set(definitions.map((item) => item.category));
  const types = new Set(definitions.map((item) => item.type));
  const items: ToolbarItem[] = [
    { category: 'all', icon: 'layers', key: 'node-menu', label: '添加', primary: true },
    { category: 'search', icon: 'layers', key: 'search', label: '节点搜索', text: '搜索' },
    { category: 'output', icon: 'folder', key: 'output', label: '素材库', text: '素材库' },
    { category: 'templates', icon: 'grid', key: 'templates', label: '模板', text: '模板' },
    { category: 'comments', icon: 'spark', key: 'comments', label: '评论', text: '评论' },
    { category: 'history', icon: 'undo', key: 'history', label: '历史', text: '历史' },
  ];
  return items.map((item) => ({
    ...item,
    disabled:
      item.disabled ||
      (MEDIA_CARD_TYPE[item.category]
        ? !types.has(MEDIA_CARD_TYPE[item.category]!)
        : !['select', 'all', 'undo', 'redo', 'style', 'erase', 'models', 'upload', 'search', 'history', 'templates', 'output', 'prompts', 'comments'].includes(item.category) &&
          !available.has(item.category)),
  }));
}

function toolClassName(active: boolean, tool: ToolbarItem): string {
  return [
    'node-library-tool',
    active ? 'is-active' : '',
    tool.primary ? 'is-primary' : '',
    tool.text ? 'has-text' : '',
  ].filter(Boolean).join(' ');
}

function toolLabel(category: string): string {
  if (category === 'all') return '添加节点';
  if (category === 'models') return '模型目录';
  if (category === 'input') return '添加节点';
  return `${category} 节点`;
}

const CONTENT_CARD_TYPES = ['input.text', 'input.image', 'input.video', 'input.audio'] as const;

export function contentCardLabel(type: string): string {
  if (type === 'input.text') return '文本';
  if (type === 'input.image') return '图片';
  if (type === 'input.video') return '视频';
  if (type === 'input.audio') return '音频';
  return type;
}

function isContentCardType(type: string): boolean {
  return (CONTENT_CARD_TYPES as readonly string[]).includes(type);
}

function groupDefinitionsByCategory(definitions: NodeDefinition[]): Map<string, NodeDefinition[]> {
  const groups = new Map<string, NodeDefinition[]>();
  for (const definition of definitions) {
    const current = groups.get(definition.category) ?? [];
    current.push(definition);
    groups.set(definition.category, current);
  }
  return groups;
}

export function filterNodeDefinitions(
  definitions: NodeDefinition[],
  query: string,
  category: string,
): NodeDefinition[] {
  return definitions.filter((definition) => {
    if (!isContentCardType(definition.type)) return false;
    const matchesCategory = category === 'all'
      || definition.category === category
      || MEDIA_CARD_TYPE[category] === definition.type;
    const search = query.trim().toLowerCase();
    const label = contentCardLabel(definition.type);
    const matchesSearch =
      !search ||
      [label, definition.title, definition.type, definition.category]
        .some((value) => value.toLowerCase().includes(search));
    return matchesCategory && matchesSearch;
  });
}
