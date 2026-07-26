import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import type { NodeCatalog, NodeDefinition } from '../types';
import { NodeLibrary, filterNodeDefinitions } from './node-library';

describe('NodeLibrary', () => {
  it('filters definitions by category, title, type, and category text', () => {
    const definitions = catalog().nodes;

    expect(filterNodeDefinitions(definitions, 'video', 'all').map((item) => item.type))
      .toEqual(['video.text_to_video']);
    expect(filterNodeDefinitions(definitions, 'prompt', 'text').map((item) => item.type))
      .toEqual(['llm.prompt_writer']);
    expect(filterNodeDefinitions(definitions, 'missing', 'all')).toEqual([]);
  });

  it('renders compact toolbar and disables editing while pending', () => {
    const markup = renderToStaticMarkup(
      <NodeLibrary
        catalog={catalog()}
        disabled={true}
        error={null}
        modelCatalog={null}
        modelCatalogError={null}
        onAddNode={() => undefined}
      />,
    );

    expect(markup).toContain('aria-label="节点工具条"');
    expect(markup).toContain('title="文本节点"');
    expect(markup).toContain('title="视频节点"');
    expect(markup).toContain('disabled=""');
  });
});

function catalog(): NodeCatalog {
  return {
    schema_version: 1,
    nodes: [
      definition('llm.prompt_writer', 'Prompt Writer', 'text'),
      definition('video.text_to_video', 'Text To Video', 'video'),
    ],
  };
}

function definition(type: string, title: string, category: string): NodeDefinition {
  return {
    type,
    title,
    category,
    provider: null,
    capability: null,
    description: title,
    inputs: [],
    outputs: [],
    params_schema: {
      required: [],
      properties: {},
      allow_unknown: false,
    },
    estimated_cost: null,
  };
}
