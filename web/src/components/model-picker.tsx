import { useEffect, useRef, useState } from 'react';
import { fetchModelCatalog } from '../api';
import { Icon } from '../icons';
import {
  displayedPickerModel,
  modelsForCapability,
  pickerModels,
  useModelPreferenceStore,
  type PickerModel,
} from '../model-picker';
import { useWorkbenchStore } from '../store';
import type { ModelCatalog, WorkbenchState } from '../types';

type ModelPickerProps = {
  catalog?: ModelCatalog | null;
  capabilityId?: string;
  providers?: WorkbenchState['providers'];
  disabled?: boolean;
};

export function ModelPicker({
  catalog: catalogProp,
  capabilityId,
  providers: providersProp,
  disabled = false,
}: ModelPickerProps) {
  const storeProviders = useWorkbenchStore((state) => state.state?.providers);
  const preferredModelId = useModelPreferenceStore((state) => state.preferredModelId);
  const setPreferredModelId = useModelPreferenceStore((state) => state.setPreferredModelId);
  const [fetchedCatalog, setFetchedCatalog] = useState<ModelCatalog | null>(null);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const catalog = catalogProp === undefined ? fetchedCatalog : catalogProp;
  const providers = providersProp ?? storeProviders;
  const models = catalog
    ? capabilityId
      ? modelsForCapability(pickerModels(catalog, providers), capabilityId)
      : pickerModels(catalog, providers)
    : [];
  const current = displayedPickerModel(models, preferredModelId);

  useEffect(() => {
    if (catalogProp !== undefined) return;
    const controller = new AbortController();
    fetchModelCatalog(controller.signal)
      .then((value) => {
        if (!controller.signal.aborted) setFetchedCatalog(value);
      })
      .catch(() => {
        if (!controller.signal.aborted) setFetchedCatalog(null);
      });
    return () => controller.abort();
  }, [catalogProp]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const root = rootRef.current;
      if (root && event.target instanceof Node && !root.contains(event.target)) {
        setOpen(false);
      }
    };
    document.addEventListener('pointerdown', onPointerDown);
    return () => document.removeEventListener('pointerdown', onPointerDown);
  }, [open]);

  return (
    <div className="model-picker" ref={rootRef}>
      <button
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-label="选择模型"
        className="model-picker-chip"
        disabled={disabled || models.length === 0}
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <Icon n="spark" s={13} />
        <span>{current?.displayName ?? (catalog ? '无可用模型' : '选择模型')}</span>
      </button>
      {open && models.length > 0 && (
        <div className="model-picker-menu" role="listbox" aria-label="模型列表">
          {models.map((model) => (
            <ModelPickerRow
              key={model.modelId}
              model={model}
              selected={model.modelId === (preferredModelId ?? current?.modelId)}
              onSelect={() => {
                setPreferredModelId(model.modelId);
                setOpen(false);
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ModelPickerRow({
  model,
  selected,
  onSelect,
}: {
  model: PickerModel;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      aria-selected={selected}
      className={selected ? 'model-picker-row model-picker-row--selected' : 'model-picker-row'}
      onClick={onSelect}
      role="option"
      type="button"
    >
      <span className="model-picker-row-title">
        <Icon n="spark" s={13} />
        {model.displayName}
      </span>
      <span className="model-picker-row-tags">
        {model.tags.map((tag) => (
          <span className="model-picker-tag" key={tag}>
            {tag}
          </span>
        ))}
      </span>
    </button>
  );
}
