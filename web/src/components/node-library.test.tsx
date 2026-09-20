import { renderToStaticMarkup } from 'react-dom/server';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { describe, expect, it } from 'vitest';
import type { NodeCatalog, NodeDefinition } from '../types';
import { NodeLibrary, filterNodeDefinitions } from './node-library';

describe('NodeLibrary', () => {
  it('filters definitions by category, title, type, and category text', () => {
    const definitions = catalog().nodes;

    expect(filterNodeDefinitions(definitions, 'video', 'all').map((item) => item.type))
      .toEqual(['input.video']);
    expect(filterNodeDefinitions(definitions, '文本', 'all').map((item) => item.type))
      .toEqual(['input.text']);
    expect(filterNodeDefinitions(definitions, 'missing', 'all')).toEqual([]);
  });

  it('hides operator types from the add catalog', () => {
    expect(filterNodeDefinitions(mediaCatalog().nodes, '', 'all').map((item) => item.type))
      .toEqual(['input.image']);
    expect(filterNodeDefinitions(mediaCatalog().nodes, 'edit', 'all')).toEqual([]);
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
    expect(markup).toContain('title="添加"');
    expect(markup).toContain('title="素材库"');
    expect(markup).toContain('title="评论"');
    expect(markup).toContain('disabled=""');
  });

  it('adds a tray item on a single click and returns to canvas selection', async () => {
    const added: string[] = [];
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <NodeLibrary
          catalog={catalog()}
          disabled={false}
          error={null}
          modelCatalog={null}
          modelCatalogError={null}
          onAddNode={(definition) => added.push(definition.type)}
        />,
      );
    });

    await act(async () => renderer!.root.findByProps({ title: '添加' }).props.onClick());
    const item = renderer!.root.findAllByProps({ className: 'node-library-item' })[0]!;
    await act(async () => item.props.onClick());

    expect(added).toEqual(['input.text']);
    expect(renderer!.root.findAllByProps({ className: 'node-library-tray' })).toHaveLength(0);
    await act(async () => renderer!.unmount());
  });

  it('adds a single image card instead of opening edit vs generate', async () => {
    const added: string[] = [];
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <NodeLibrary
          catalog={mediaCatalog()}
          disabled={false}
          error={null}
          modelCatalog={null}
          modelCatalogError={null}
          onAddNode={(definition) => added.push(definition.type)}
        />,
      );
    });

    await act(async () => renderer!.root.findByProps({ title: '添加' }).props.onClick());
    const item = renderer!.root.findAllByProps({ className: 'node-library-item' }).find((entry) =>
      String(entry.children).includes('图片') || String(entry.props.children).includes('图片'),
    );
    await act(async () => {
      const target = item ?? renderer!.root.findAllByProps({ className: 'node-library-item' })[0]!;
      target.props.onClick();
    });

    expect(added).toEqual(['input.image']);
    expect(renderer!.root.findAllByProps({ className: 'node-library-tray' })).toHaveLength(0);
    await act(async () => renderer!.unmount());
  });
});

function catalog(): NodeCatalog {
  return {
    schema_version: 1,
    nodes: [
      definition('input.text', 'Text Input', 'input'),
      definition('input.video', 'Video Input', 'input'),
      definition('llm.prompt_writer', 'Prompt Writer', 'text'),
      definition('video.text_to_video', 'Text To Video', 'video'),
    ],
  };
}

function mediaCatalog(): NodeCatalog {
  return {
    schema_version: 1,
    nodes: [
      definition('input.image', 'Image Input', 'input'),
      definition('image.edit', 'Edit Image', 'image'),
      definition('image.generate', 'Generate Image', 'image'),
      definition('video.text_to_video', 'Text To Video', 'video'),
      definition('video.image_to_video', 'Image To Video', 'video'),
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
