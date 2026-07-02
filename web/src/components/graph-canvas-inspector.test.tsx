import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createManualWorkspaceProposal } from '../api';
import type { GraphNodeState, NodeDefinition, WorkflowGraph } from '../types';
import {
  GraphInspector,
  controlKindForParam,
  validateParamDraft,
} from './graph-canvas-inspector';

describe('GraphInspector', () => {
  afterEach(() => {
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
        onSetParam={async () => undefined}
        workflowNode={workflowNode()}
      />,
    );

    expect(markup).toContain('duration_sec');
    expect(markup).toContain('type="number"');
    expect(markup).toContain('<select');
    expect(markup).toContain('9:16');
    expect(markup).toContain('title="随机 seed"');
    expect(markup).toContain('保存');
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
