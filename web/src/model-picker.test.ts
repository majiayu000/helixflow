import { afterEach, describe, expect, it } from 'vitest';
import {
  canvasContextWithPreferredModel,
  displayedPickerModel,
  modelsForCapability,
  pickerModels,
  useModelPreferenceStore,
} from './model-picker';
import type { ModelCatalog } from './types';

function catalog(): ModelCatalog {
  return {
    catalogRevision: 'sha256:test',
    capabilities: [
      { capabilityId: 'text_to_image', category: 'image', displayName: 'Text To Image' },
      { capabilityId: 'text_to_video', category: 'video', displayName: 'Text To Video' },
    ],
    models: [
      {
        modelId: 'google/nano-banana-2',
        familyId: 'nano-banana',
        displayName: 'Nano Banana 2',
        vendor: 'google',
        lifecycle: 'active',
        aliases: ['nano banana'],
      },
      {
        modelId: 'bytedance/seedance-v1.5-pro',
        familyId: 'seedance',
        displayName: 'Seedance 1.5 Pro',
        vendor: 'bytedance',
        lifecycle: 'active',
        aliases: ['seedance'],
      },
    ],
    bindings: [
      {
        bindingId: 'nano.atlas.v1',
        capabilityId: 'text_to_image',
        modelId: 'google/nano-banana-2',
        mode: 'text_to_image',
        implementation: { apiConnector: { connectorId: 'atlas', operationId: 'op-a' } },
        bindingRevision: 'v1',
        availability: 'enabled',
      },
      {
        bindingId: 'seedance.atlas.v1',
        capabilityId: 'text_to_video',
        modelId: 'bytedance/seedance-v1.5-pro',
        mode: 'text_to_video',
        implementation: { apiConnector: { connectorId: 'atlas', operationId: 'op-c' } },
        bindingRevision: 'v1',
        availability: 'enabled',
      },
    ],
    defaultBindings: { text_to_image: 'nano.atlas.v1' },
  };
}

describe('model picker projection', () => {
  afterEach(() => {
    useModelPreferenceStore.getState().setPreferredModelId(null);
  });

  it('lists enabled models with capability tags and a default', () => {
    const models = pickerModels(catalog());
    expect(models.map((model) => model.modelId)).toEqual([
      'google/nano-banana-2',
      'bytedance/seedance-v1.5-pro',
    ]);
    expect(models[0]).toMatchObject({
      displayName: 'Nano Banana 2',
      tags: ['文生图'],
      isDefault: true,
    });
    expect(models[1]?.tags).toEqual(['文生视频']);
  });

  it('hides models whose bindings are not ready for the current provider', () => {
    const models = pickerModels(catalog(), {
      defaultProvider: 'mock',
      selectedProvider: 'mock',
      runtimeProviders: [],
      workflowBackends: [],
      apiConnectors: [],
      capabilityReadiness: [
        {
          capabilityId: 'text_to_image',
          providerId: 'mock',
          runnable: true,
          mode: 'direct',
        },
      ],
    });
    expect(models).toEqual([]);
  });

  it('keeps only models tagged for a capability', () => {
    const models = pickerModels(catalog());
    expect(modelsForCapability(models, 'text_to_image').map((model) => model.modelId)).toEqual([
      'google/nano-banana-2',
    ]);
    expect(modelsForCapability(models, 'text_to_video').map((model) => model.modelId)).toEqual([
      'bytedance/seedance-v1.5-pro',
    ]);
  });

  it('prefers an explicit user pick over the catalog default', () => {
    const models = pickerModels(catalog());
    expect(displayedPickerModel(models, 'bytedance/seedance-v1.5-pro')?.modelId).toBe(
      'bytedance/seedance-v1.5-pro',
    );
    expect(displayedPickerModel(models, null)?.modelId).toBe('google/nano-banana-2');
  });

  it('only attaches requestedModel after an explicit user pick', () => {
    expect(canvasContextWithPreferredModel(['n1'])).toEqual({
      selection: { nodeIds: ['n1'] },
    });
    useModelPreferenceStore.getState().setPreferredModelId('google/nano-banana-2');
    expect(canvasContextWithPreferredModel(['n1'])).toEqual({
      selection: { nodeIds: ['n1'] },
      requestedModel: 'google/nano-banana-2',
    });
  });
});
