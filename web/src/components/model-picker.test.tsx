import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ModelPicker } from './model-picker';
import type { ModelCatalog } from '../types';

function catalog(): ModelCatalog {
  return {
    catalogRevision: 'sha256:test',
    capabilities: [
      { capabilityId: 'text_to_image', category: 'image', displayName: 'Text To Image' },
    ],
    models: [
      {
        modelId: 'google/nano-banana-2',
        familyId: 'nano-banana',
        displayName: 'Nano Banana 2',
        vendor: 'google',
        lifecycle: 'active',
        aliases: [],
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
    ],
    defaultBindings: { text_to_image: 'nano.atlas.v1' },
  };
}

describe('ModelPicker', () => {
  it('renders the current model chip without lock or paywall copy', () => {
    const markup = renderToStaticMarkup(<ModelPicker catalog={catalog()} />);
    expect(markup).toContain('aria-label="选择模型"');
    expect(markup).toContain('Nano Banana 2');
    expect(markup).not.toContain('解锁');
    expect(markup).not.toContain('付费');
    expect(markup).not.toContain('model-picker-row');
  });
});
