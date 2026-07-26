import { useMemo, useState } from 'react';
import type { ModelCatalog, NodeCatalog, NodeDefinition } from '../types';

type ModelCatalogTrayProps = {
  modelCatalog: ModelCatalog | null;
  nodeCatalog: NodeCatalog | null;
  error: string | null;
  disabled: boolean;
  onAddNode: (definition: NodeDefinition) => void;
};

type CatalogView = 'byCapability' | 'byModel';

/// Two projections of the same catalog facts (tech.md §12): browse
/// capabilities and expand the models that implement them, or browse models
/// and expand their capabilities. The tree is a projection — adding an entry
/// simply adds the executable node for that capability.
export function ModelCatalogTray({
  modelCatalog,
  nodeCatalog,
  error,
  disabled,
  onAddNode,
}: ModelCatalogTrayProps) {
  const [view, setView] = useState<CatalogView>('byCapability');
  const definitionByCanonicalCapability = useMemo(() => {
    const map = new Map<string, NodeDefinition>();
    for (const definition of nodeCatalog?.nodes ?? []) {
      if (definition.capability) {
        map.set(canonicalCapability(definition.capability), definition);
      }
    }
    return map;
  }, [nodeCatalog]);

  if (error) {
    return <div className="node-library-empty">{error}</div>;
  }
  if (!modelCatalog) {
    return <div className="node-library-empty">模型目录加载中…</div>;
  }

  const groups =
    view === 'byCapability'
      ? capabilityGroups(modelCatalog)
      : modelGroups(modelCatalog);

  return (
    <div className="model-catalog" data-view={view}>
      <div className="model-catalog-tabs" role="tablist" aria-label="目录视图">
        <button
          aria-selected={view === 'byCapability'}
          className="model-catalog-tab"
          onClick={() => setView('byCapability')}
          role="tab"
          type="button"
        >
          按能力
        </button>
        <button
          aria-selected={view === 'byModel'}
          className="model-catalog-tab"
          onClick={() => setView('byModel')}
          role="tab"
          type="button"
        >
          按模型
        </button>
      </div>
      <div className="node-library-list">
        {groups.length === 0 && <div className="node-library-empty">目录为空</div>}
        {groups.map((group) => (
          <div className="model-catalog-group" key={group.key}>
            <div className="model-catalog-group-title">{group.title}</div>
            {group.entries.map((entry) => {
              const definition = definitionByCanonicalCapability.get(entry.capabilityId);
              return (
                <button
                  className="node-library-item"
                  disabled={disabled || !definition}
                  key={entry.key}
                  onDoubleClick={() => definition && onAddNode(definition)}
                  title={definition ? `双击添加 ${definition.title}` : '当前运行时不支持该能力'}
                  type="button"
                >
                  <span className="node-library-title">{entry.label}</span>
                  <span className="node-library-type">
                    {entry.detail}
                    {entry.isDefault ? ' · 默认' : ''}
                  </span>
                </button>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}

type CatalogEntry = {
  key: string;
  label: string;
  detail: string;
  capabilityId: string;
  isDefault: boolean;
};

type CatalogGroup = { key: string; title: string; entries: CatalogEntry[] };

export function capabilityGroups(catalog: ModelCatalog): CatalogGroup[] {
  return catalog.capabilities
    .map((capability) => {
      const entries = catalog.bindings
        .filter((binding) => binding.capabilityId === capability.capabilityId)
        .map((binding) => {
          const model = catalog.models.find((item) => item.modelId === binding.modelId);
          return {
            key: binding.bindingId,
            label: model?.displayName ?? binding.modelId,
            detail: connectorLabel(binding),
            capabilityId: binding.capabilityId,
            isDefault:
              catalog.defaultBindings[binding.capabilityId] === binding.bindingId,
          };
        });
      return { key: capability.capabilityId, title: capability.displayName, entries };
    })
    .filter((group) => group.entries.length > 0);
}

export function modelGroups(catalog: ModelCatalog): CatalogGroup[] {
  return catalog.models
    .map((model) => {
      const entries = catalog.bindings
        .filter((binding) => binding.modelId === model.modelId)
        .map((binding) => {
          const capability = catalog.capabilities.find(
            (item) => item.capabilityId === binding.capabilityId,
          );
          return {
            key: binding.bindingId,
            label: capability?.displayName ?? binding.capabilityId,
            detail: connectorLabel(binding),
            capabilityId: binding.capabilityId,
            isDefault:
              catalog.defaultBindings[binding.capabilityId] === binding.bindingId,
          };
        });
      return { key: model.modelId, title: `${model.displayName} (${model.vendor})`, entries };
    })
    .filter((group) => group.entries.length > 0);
}

/// Mirrors the backend legacy→canonical capability rename. Identity for live
/// data since GH145; deleted with the backend map in stage B (#145).
export function canonicalCapability(legacy: string): string {
  return legacy === 'image_generate' ? 'text_to_image' : legacy;
}

function connectorLabel(binding: ModelCatalog['bindings'][number]): string {
  if ('apiConnector' in binding.implementation) {
    return `经 ${binding.implementation.apiConnector.connectorId}`;
  }
  return `模板 ${binding.implementation.workflowTemplate.backendId}`;
}
