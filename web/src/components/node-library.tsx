import { useMemo, useState } from 'react';
import { Icon, Port, type IconName } from '../icons';
import type { NodeCatalog, NodeDefinition } from '../types';

type NodeLibraryProps = {
  catalog: NodeCatalog | null;
  error: string | null;
  disabled: boolean;
  onAddNode: (definition: NodeDefinition) => void;
};

export function NodeLibrary({ catalog, error, disabled, onAddNode }: NodeLibraryProps) {
  const [query, setQuery] = useState('');
  const [category, setCategory] = useState('select');
  const [open, setOpen] = useState(false);
  const definitions = catalog?.nodes ?? [];
  const categories = useMemo(
    () => Array.from(new Set(definitions.map((item) => item.category))).sort(),
    [definitions],
  );
  const tools = useMemo(() => toolbarItems(categories), [categories]);
  const activeCategory = category === 'select' ? 'all' : category;
  const filtered = useMemo(
    () => filterNodeDefinitions(definitions, query, activeCategory),
    [activeCategory, definitions, query],
  );
  const showTray = open && category !== 'select';
  const visibleItems = query.trim() ? filtered : filtered.slice(0, 8);
  const definitionsByCategory = useMemo(() => groupDefinitionsByCategory(definitions), [definitions]);

  const selectTool = (tool: ToolbarItem) => {
    if (tool.category === 'select') {
      setCategory('select');
      setOpen(false);
      return;
    }

    setQuery('');
    setCategory(tool.category);

    const categoryDefinitions =
      tool.category === 'all' ? definitions : definitionsByCategory.get(tool.category) ?? [];
    if (!disabled && tool.category !== 'all' && categoryDefinitions.length === 1) {
      setOpen(false);
      onAddNode(categoryDefinitions[0]!);
      return;
    }

    setOpen(true);
  };

  return (
    <aside className="node-library" onPointerDown={(event) => event.stopPropagation()}>
      <div className="node-library-bar" role="toolbar" aria-label="节点工具条">
        {tools.map((tool, index) => (
          <FragmentedToolButton
            active={tool.category === category}
            disabled={Boolean(tool.disabled)}
            key={tool.key}
            leadingDivider={index === 3 || index === 7 || index === tools.length - 1}
            onClick={() => selectTool(tool)}
            tool={tool}
          />
        ))}
      </div>
      {showTray && (
        <div className="node-library-tray">
          <div className="node-library-tray-head">
            <strong>{toolLabel(category)}</strong>
            {disabled && <span>待处理变更</span>}
          </div>
          <input
            aria-label="搜索节点"
            className="node-library-search"
            disabled={disabled}
            placeholder="搜索节点"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          {error && <div className="node-library-empty">{error}</div>}
          {!error && definitions.length === 0 && <div className="node-library-empty">暂无节点</div>}
          {!error && definitions.length > 0 && filtered.length === 0 && (
            <div className="node-library-empty">无匹配节点</div>
          )}
          {!error && filtered.length > 0 && (
            <div className="node-library-list">
              {visibleItems.map((definition) => (
                <button
                  className="node-library-item"
                  disabled={disabled}
                  draggable={!disabled}
                  key={definition.type}
                  onDoubleClick={() => onAddNode(definition)}
                  onClick={() => undefined}
                  onDragStart={(event) => {
                    event.dataTransfer.effectAllowed = 'copy';
                    event.dataTransfer.setData('application/x-helixflow-node-type', definition.type);
                  }}
                  type="button"
                >
                  <span className="node-library-title">
                    <Port type={definition.category} />
                    {definition.title}
                  </span>
                  <span className="node-library-type">{definition.type}</span>
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
  disabled?: boolean;
  icon: IconName;
  key: string;
  label: string;
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
        className={active ? 'node-library-tool is-active' : 'node-library-tool'}
        disabled={disabled}
        onClick={onClick}
        title={tool.label}
        type="button"
      >
        <Icon n={tool.icon} s={18} sw={1.9} />
      </button>
    </>
  );
}

function toolbarItems(categories: string[]): ToolbarItem[] {
  const available = new Set(categories);
  const items: ToolbarItem[] = [
    { category: 'select', icon: 'hand', key: 'select', label: '选择画布' },
    { category: 'undo', disabled: true, icon: 'undo', key: 'undo', label: '撤销' },
    { category: 'redo', disabled: true, icon: 'redo', key: 'redo', label: '重做' },
    { category: 'text', icon: 'text', key: 'text', label: '文本节点' },
    { category: 'image', icon: 'image', key: 'image', label: '图像节点' },
    { category: 'video', icon: 'play', key: 'video', label: '视频节点' },
    { category: 'audio', icon: 'music', key: 'audio', label: '音频节点' },
    { category: 'all', icon: 'sliders', key: 'all', label: '全部节点' },
    { category: 'input', icon: 'export', key: 'input', label: '输入节点' },
    { category: 'output', icon: 'folder', key: 'output', label: '输出节点' },
    { category: 'style', disabled: true, icon: 'palette', key: 'style', label: '样式' },
    { category: 'erase', disabled: true, icon: 'eraser', key: 'erase', label: '清除' },
  ];
  return items.map((item) => ({
    ...item,
    disabled:
      item.disabled ||
      (!['select', 'all', 'undo', 'redo', 'style', 'erase'].includes(item.category) &&
        !available.has(item.category)),
  }));
}

function toolLabel(category: string): string {
  if (category === 'all') return '全部节点';
  if (category === 'input') return '输入节点';
  if (category === 'output') return '输出节点';
  if (category === 'text') return '文本节点';
  if (category === 'image') return '图像节点';
  if (category === 'video') return '视频节点';
  return `${category} 节点`;
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
    const matchesCategory = category === 'all' || definition.category === category;
    const search = query.trim().toLowerCase();
    const matchesSearch =
      !search ||
      [definition.title, definition.type, definition.category]
        .some((value) => value.toLowerCase().includes(search));
    return matchesCategory && matchesSearch;
  });
}
