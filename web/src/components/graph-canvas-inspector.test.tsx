import { renderToStaticMarkup } from 'react-dom/server';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createManualWorkspaceProposal } from '../api';
import type { GraphNodeState, NodeDefinition, WorkflowGraph } from '../types';
import {
  GraphInspector,
  controlKindForParam,
  validateParamDraft,
} from './graph-canvas-inspector';

describe('GraphInspector', () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(async () => {
    if (renderer) await act(async () => renderer?.unmount());
    renderer = null;
    vi.unstubAllGlobals();
  });

  it('maps schema controls and validates draft values locally', () => {
    const definition = videoDefinition();
    const duration = definition.params_schema.properties.duration_sec;
    const aspect = definition.params_schema.properties.aspect_ratio;
    const seed = definition.params_schema.properties.seed;

    expect(controlKindForParam(definition.params_schema.properties.prompt)).toBe('text');
    expect(controlKindForParam(duration)).toBe('number');
    expect(controlKindForParam(aspect)).toBe('select');
    expect(controlKindForParam(seed)).toBe('number');
    expect(controlKindForParam(undefined)).toBe('readonly');
    expect(validateParamDraft('duration_sec', '6', duration, true)).toEqual({
      ok: true,
      value: 6,
    });
    expect(validateParamDraft('duration_sec', '11', duration, true)).toEqual({
      ok: false,
      error: 'duration_sec 不能大于 10',
    });
    expect(validateParamDraft('seed', '', seed, false)).toEqual({
      ok: false,
      error: 'seed 请输入值',
    });
    expect(validateParamDraft('aspect_ratio', JSON.stringify('4:3'), aspect, true)).toEqual({
      ok: false,
      error: 'aspect_ratio 不在允许选项内',
    });
  });

  it('renders editable controls from node catalog schema', () => {
    const markup = renderToStaticMarkup(
      <GraphInspector
        definition={videoDefinition()}
        node={videoNode()}
        onClose={() => {}}
        onRequestProposal={async () => undefined}
        onSetParam={async () => undefined}
        workflowNode={workflowNode()}
      />,
    );

    expect(markup).toContain('duration_sec');
    expect(markup).toContain('type="number"');
    expect(markup).toContain('<select');
    expect(markup).toContain('9:16');
    expect(markup).toContain('title="随机 seed"');
    expect(markup).toContain('Ask Agent for node proposal');
    expect(markup).toContain('保存');
    expect(markup).not.toContain('宫格切分');
    expect(markup).not.toContain('生成式修图');
  });

  it('exposes 宫格切分 only when a split handler is provided', () => {
    const markup = renderToStaticMarkup(
      <GraphInspector
        definition={videoDefinition()}
        node={videoNode()}
        onClose={() => {}}
        onSplitImageGrid={async () => undefined}
        workflowNode={workflowNode()}
      />,
    );
    expect(markup).toContain('宫格切分');
    expect(markup).toContain('切成 9 张');
  });

  it('exposes 生成式修图 only when an image-edit handler is provided', () => {
    const markup = renderToStaticMarkup(
      <GraphInspector
        definition={videoDefinition()}
        node={videoNode()}
        onClose={() => {}}
        onApplyImageCanvasTool={async () => undefined}
        workspaceId="ws_1"
        workflowNode={workflowNode()}
      />,
    );
    expect(markup).toContain('生成式修图');
    expect(markup).toContain('扩图');
    expect(markup).toContain('擦除');
    expect(markup).toContain('抠图');
    expect(markup).toContain('超分');
    expect(markup).toContain('增强');
    expect(markup).not.toContain('宫格切分');
  });

  it('preserves bounded ops opIndex errors for inline callers', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => jsonResponse({ error: 'invalid param', opIndex: 0 }, 400)),
    );

    await expect(
      createManualWorkspaceProposal('ws_test', {
        baseVersionId: 'ver_test_1',
        ops: [{ op: 'set_param', id: 'video', key: 'duration_sec', value: 'slow' }],
      }),
    ).rejects.toThrow('invalid param (op 0)');
  });

  it('keeps parameter disclosure pointer events inside the inspector', async () => {
    await act(async () => {
      renderer = create(
        <GraphInspector
          definition={videoDefinition()}
          node={videoNode()}
          onClose={() => {}}
          workflowNode={workflowNode()}
        />,
      );
    });
    if (!renderer) throw new Error('renderer was not created');
    const summary = renderer.root.findByType('summary');
    const pointerEvent = { stopPropagation: vi.fn() };
    const clickEvent = { stopPropagation: vi.fn() };

    await act(async () => {
      summary.props.onPointerDown(pointerEvent);
      summary.props.onClick(clickEvent);
    });

    expect(pointerEvent.stopPropagation).toHaveBeenCalledOnce();
    expect(clickEvent.stopPropagation).toHaveBeenCalledOnce();
  });
});

function videoNode(): GraphNodeState {
  return {
    id: 'video',
    nodeType: 'video.text_to_video',
    title: 'Video render',
    category: 'Video',
    status: 'queued',
    position: { x: 486, y: 156 },
    provider: 'mock',
    summary: '9:16, 4 seconds',
  };
}

function workflowNode(): WorkflowGraph['nodes'][string] {
  return {
    node_type: 'video.text_to_video',
    title: 'Video render',
    params: {
      prompt: 'clean product shot',
      duration_sec: 4,
      aspect_ratio: '9:16',
    },
    pos: [486, 156],
  };
}

describe('GraphInspector implementation section', () => {
  it('shows the resolved implementation for capability nodes', () => {
    const markup = renderToStaticMarkup(
      <GraphInspector
        definition={videoDefinition()}
        node={videoNode()}
        onClose={() => {}}
        resolution={{
          status: 'resolved',
          resolved: {
            capabilityId: 'text_to_video',
            requestedModelId: null,
            resolvedModelId: 'bytedance/seedance-v1.5-pro',
            bindingId: 'bytedance.seedance-v1-5-pro.text-to-video.atlas.v1',
            bindingRevision: 'v1',
            target: {
              apiConnector: { connectorId: 'atlas', operationId: 'op' },
            },
          },
        }}
        workflowNode={workflowNode()}
      />,
    );

    expect(markup).toContain('bytedance/seedance-v1.5-pro');
    expect(markup).toContain('bytedance.seedance-v1-5-pro.text-to-video.atlas.v1');
    expect(markup).toContain('atlas');
  });

  it('shows a stable reason when the implementation cannot resolve', () => {
    const markup = renderToStaticMarkup(
      <GraphInspector
        definition={videoDefinition()}
        node={videoNode()}
        onClose={() => {}}
        resolution={{
          status: 'unresolvable',
          code: 'BINDING_NOT_FOUND',
          message: 'no binding implements capability `text_to_video`',
          recoverable: false,
        }}
        workflowNode={workflowNode()}
      />,
    );

    expect(markup).toContain('[BINDING_NOT_FOUND]');
    expect(markup).toContain('不可运行');
  });
});

function videoDefinition(): NodeDefinition {
  return {
    type: 'video.text_to_video',
    title: 'Text To Video',
    category: 'video',
    provider: 'mock',
    capability: 'text_to_video',
    description: 'Generates a deterministic placeholder video artifact.',
    inputs: [{ name: 'prompt', type: 'TEXT', required: true }],
    outputs: [{ name: 'video', type: 'VIDEO', required: true }],
    params_schema: {
      required: ['prompt', 'duration_sec', 'aspect_ratio'],
      properties: {
        prompt: { type: 'string', enum_values: [], minimum: null, maximum: null },
        duration_sec: { type: 'integer', enum_values: [], minimum: 1, maximum: 10 },
        seed: { type: 'integer', enum_values: [], minimum: 0, maximum: 999999 },
        aspect_ratio: {
          type: 'string',
          enum_values: ['1:1', '9:16', '16:9'],
          minimum: null,
          maximum: null,
        },
      },
      allow_unknown: false,
    },
    estimated_cost: { unit: 'call', catalog_key: 'mock.text_to_video' },
  };
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}
