import { useMemo, useState } from 'react';
import { Port } from '../icons';
import type { NodeCatalog, NodeDefinition } from '../types';

type NodeLibraryProps = {
  catalog: NodeCatalog | null;
  error: string | null;
  disabled: boolean;
  onAddNode: (definition: NodeDefinition) => void;
};

export function NodeLibrary({ catalog, error, disabled, onAddNode }: NodeLibraryProps) {
  const [query, setQuery] = useState('');
  const [category, setCategory] = useState('all');
  const definitions = catalog?.nodes ?? [];
  const categories = useMemo(
    () => ['all', ...Array.from(new Set(definitions.map((item) => item.category))).sort()],
    [definitions],
  );
  const filtered = useMemo(
    () => filterNodeDefinitions(definitions, query, category),
    [category, definitions, query],
  );

  return (
    <aside className="node-library" onPointerDown={(event) => event.stopPropagation()}>
      <div className="node-library-head">
        <strong>节点库</strong>
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
      <div className="node-library-tabs" role="tablist" aria-label="节点类别">
        {categories.map((item) => (
          <button
            className={item === category ? 'on' : ''}
            disabled={disabled}
            key={item}
            onClick={() => setCategory(item)}
            type="button"
          >
            {item === 'all' ? '全部' : item}
          </button>
        ))}
      </div>
      {error && <div className="node-library-empty">{error}</div>}
      {!error && definitions.length === 0 && <div className="node-library-empty">暂无节点</div>}
      {!error && definitions.length > 0 && filtered.length === 0 && (
        <div className="node-library-empty">无匹配节点</div>
      )}
      <div className="node-library-list">
        {filtered.map((definition) => (
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
    </aside>
  );
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
