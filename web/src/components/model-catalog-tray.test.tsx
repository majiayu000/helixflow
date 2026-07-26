import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import {
  ModelCatalogTray,
  canonicalCapability,
  capabilityGroups,
  modelGroups,
} from './model-catalog-tray';
import type { ModelCatalog, NodeCatalog } from '../types';

function modelCatalog(): ModelCatalog {
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
      },
      {
        bindingId: 'nano.fal.v1',
        capabilityId: 'text_to_image',
        modelId: 'google/nano-banana-2',
        mode: 'text_to_image',
        implementation: { apiConnector: { connectorId: 'fal', operationId: 'op-b' } },
        bindingRevision: 'v1',
      },
      {
        bindingId: 'seedance.atlas.v1',
        capabilityId: 'text_to_video',
        modelId: 'bytedance/seedance-v1.5-pro',
        mode: 'text_to_video',
        implementation: { apiConnector: { connectorId: 'atlas', operationId: 'op-c' } },
        bindingRevision: 'v1',
      },
    ],
    defaultBindings: { text_to_image: 'nano.atlas.v1' },
  };
}

function nodeCatalog(): NodeCatalog {
  return {
    schema_version: 1,
    nodes: [
      {
        type: 'image.generate',
        title: 'Generate Image',
        category: 'image',
        provider: null,
        capability: 'image_generate',
        description: '',
        inputs: [],
        outputs: [],
        params_schema: { required: [], properties: {}, allow_unknown: false },
        estimated_cost: null,
      },
    ],
  };
}

describe('model catalog projections', () => {
  it('maps the legacy capability rename', () => {
    expect(canonicalCapability('image_generate')).toBe('text_to_image');
    expect(canonicalCapability('text_to_video')).toBe('text_to_video');
  });

  it('groups by capability with default markers and connectors', () => {
    const groups = capabilityGroups(modelCatalog());
    const image = groups.find((group) => group.key === 'text_to_image');
    expect(image?.entries.map((entry) => entry.detail)).toEqual(['经 atlas', '经 fal']);
    expect(image?.entries.map((entry) => entry.isDefault)).toEqual([true, false]);
  });

  it('groups by model with capabilities as entries', () => {
    const groups = modelGroups(modelCatalog());
    const nano = groups.find((group) => group.key === 'google/nano-banana-2');
    expect(nano?.title).toContain('Nano Banana 2');
    expect(nano?.entries.every((entry) => entry.capabilityId === 'text_to_image')).toBe(true);
  });

  it('renders both projections from the same facts', () => {
    const markup = renderToStaticMarkup(
      <ModelCatalogTray
        disabled={false}
        error={null}
        modelCatalog={modelCatalog()}
        nodeCatalog={nodeCatalog()}
        onAddNode={() => undefined}
      />,
    );

    expect(markup).toContain('按能力');
    expect(markup).toContain('按模型');
    expect(markup).toContain('Text To Image');
    expect(markup).toContain('Nano Banana 2');
    expect(markup).toContain('· 默认');
  });

  it('disables entries whose capability has no executable node', () => {
    const markup = renderToStaticMarkup(
      <ModelCatalogTray
        disabled={false}
        error={null}
        modelCatalog={modelCatalog()}
        nodeCatalog={nodeCatalog()}
        onAddNode={() => undefined}
      />,
    );

    // text_to_video has no node definition in the fixture catalog.
    expect(markup).toContain('当前运行时不支持该能力');
  });
});
