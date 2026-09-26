import { useMemo, useState } from 'react';
import { catalogBindingReady } from '../model-picker';
import type { ModelCatalog, NodeCatalog, NodeDefinition, WorkbenchState } from '../types';

type ModelCatalogTrayProps = {
  modelCatalog: ModelCatalog | null;
  nodeCatalog: NodeCatalog | null;
  error: string | null;
  disabled: boolean;
  providers?: WorkbenchState['providers'];
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
  providers,
  onAddNode,
}: ModelCatalogTrayProps) {
  const [view, setView] = useState<CatalogView>('byCapability');
  const definitionByCanonicalCapability = useMemo(() => {
    const map = new Map<string, NodeDefinition>();
    for (const definition of nodeCatalog?.nodes ?? []) {
      if (definition.capability) {
        map.set(definition.capability, definition);
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
      ? capabilityGroups(modelCatalog, providers)
      : modelGroups(modelCatalog, providers);

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
        {groups.length === 0 && (
          <div className="node-library-empty">
            {providers
              ? `当前 provider ${providers.selectedProvider} 没有可用的模型目录绑定`
              : '目录为空'}
          </div>
        )}
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

export function capabilityGroups(
  catalog: ModelCatalog,
  providers?: WorkbenchState['providers'],
): CatalogGroup[] {
  return catalog.capabilities
    .map((capability) => {
      const entries = catalog.bindings
        .filter(
          (binding) =>
            binding.availability === 'enabled' &&
            binding.capabilityId === capability.capabilityId && catalogBindingReady(binding, providers),
        )
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

export function modelGroups(
  catalog: ModelCatalog,
  providers?: WorkbenchState['providers'],
): CatalogGroup[] {
  return catalog.models
    .map((model) => {
      const entries = catalog.bindings
        .filter(
          (binding) =>
            binding.availability === 'enabled' &&
            binding.modelId === model.modelId &&
            catalogBindingReady(binding, providers),
        )
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

function connectorLabel(binding: ModelCatalog['bindings'][number]): string {
  if ('apiConnector' in binding.implementation) {
    return `经 ${binding.implementation.apiConnector.connectorId}`;
  }
  return `模板 ${binding.implementation.workflowTemplate.backendId}`;
}
