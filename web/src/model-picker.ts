import { create } from 'zustand';
import type { ModelCatalog, WorkbenchState } from './types';

const STORAGE_KEY = 'helixflow.preferred-model-id';
const CAPABILITY_TAGS: Record<string, string> = {
  prompt_writer: '提示词',
  text_to_image: '文生图',
  image_edit: '修图',
  text_to_video: '文生视频',
  image_to_video: '图生视频',
  video_extend: '视频延长',
  upscale_image: '放大',
  upscale_video: '视频放大',
  image_analyze: '识图',
};

export type PickerModel = {
  modelId: string;
  displayName: string;
  vendor: string;
  tags: string[];
  isDefault: boolean;
};

type ModelPreferenceState = {
  preferredModelId: string | null;
  setPreferredModelId: (id: string | null) => void;
};

export const useModelPreferenceStore = create<ModelPreferenceState>((set) => ({
  preferredModelId: readStoredPreferredModelId(),
  setPreferredModelId: (id) => {
    writeStoredPreferredModelId(id);
    set({ preferredModelId: id });
  },
}));

export function pickerModels(
  catalog: ModelCatalog,
  providers?: WorkbenchState['providers'],
): PickerModel[] {
  const defaultModelIds = new Set(
    Object.values(catalog.defaultBindings)
      .map((bindingId) => catalog.bindings.find((binding) => binding.bindingId === bindingId)?.modelId)
      .filter((modelId): modelId is string => Boolean(modelId)),
  );
  return catalog.models.flatMap((model) => {
    const bindings = catalog.bindings.filter(
      (binding) =>
        binding.availability === 'enabled' &&
        binding.modelId === model.modelId &&
        catalogBindingReady(binding, providers),
    );
    if (bindings.length === 0) return [];
    const tags = unique(
      bindings.map(
        (binding) =>
          CAPABILITY_TAGS[binding.capabilityId] ??
          catalog.capabilities.find((item) => item.capabilityId === binding.capabilityId)?.displayName ??
          binding.capabilityId,
      ),
    );
    return [
      {
        modelId: model.modelId,
        displayName: model.displayName,
        vendor: model.vendor,
        tags,
        isDefault: defaultModelIds.has(model.modelId),
      },
    ];
  });
}

export function catalogBindingReady(
  binding: ModelCatalog['bindings'][number],
  providers?: WorkbenchState['providers'],
): boolean {
  if (!providers?.capabilityReadiness) return true;
  const readiness = providers.capabilityReadiness.find(
    (item) => item.capabilityId === binding.capabilityId,
  );
  if (!readiness?.runnable || readiness.mode !== 'catalog') return false;
  if ('apiConnector' in binding.implementation) {
    return binding.implementation.apiConnector.connectorId === providers.selectedProvider;
  }
  return false;
}

export function modelsForCapability(
  models: PickerModel[],
  capabilityId: string,
): PickerModel[] {
  const tag = CAPABILITY_TAGS[capabilityId];
  if (!tag) return models;
  return models.filter((model) => model.tags.includes(tag));
}

export function canvasContextWithPreferredModel(nodeIds: string[]): {
  selection: { nodeIds: string[] };
  requestedModel?: string;
} {
  const requestedModel = useModelPreferenceStore.getState().preferredModelId?.trim();
  if (requestedModel && requestedModel.length > 0 && requestedModel.length <= 96) {
    return { selection: { nodeIds }, requestedModel };
  }
  return { selection: { nodeIds } };
}

export function displayedPickerModel(
  models: PickerModel[],
  preferredModelId: string | null,
): PickerModel | null {
  if (preferredModelId) {
    const preferred = models.find((model) => model.modelId === preferredModelId);
    if (preferred) return preferred;
  }
  return models.find((model) => model.isDefault) ?? models[0] ?? null;
}

function readStoredPreferredModelId(): string | null {
  if (typeof localStorage === 'undefined') return null;
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    return value && value.trim().length > 0 ? value : null;
  } catch {
    return null;
  }
}

function writeStoredPreferredModelId(id: string | null): void {
  if (typeof localStorage === 'undefined') return;
  try {
    if (id && id.trim().length > 0) localStorage.setItem(STORAGE_KEY, id);
    else localStorage.removeItem(STORAGE_KEY);
  } catch {
    return;
  }
}

function unique(values: string[]): string[] {
  return [...new Set(values)];
}
