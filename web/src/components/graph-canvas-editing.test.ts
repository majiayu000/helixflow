import { afterEach, describe, expect, it, vi } from 'vitest';
import { canvasCapabilities } from './graph-canvas-capabilities';
import {
  createCanvasEditActions,
  nextAvailableNodePosition,
} from './graph-canvas-edit-actions';
import type { ManualProposalInput, NodeDefinition, WorkbenchState, WorkflowGraph } from '../types';
import {
  buildAddNodeProposalInput,
  buildDuplicateNodeProposalInput,
  buildCropResultProposalInput,
  buildDeleteNodesProposalInput,
  buildGridSplitProposalInput,
  buildImageCanvasToolResultProposalInput,
  buildMediaGenerateProposalInput,
  buildVideoFrameProposalInput,
  fanInConnectFrom,
  mediaGenerateTarget,
  videoFrameLabel,
  buildMediaIngestProposalInput,
  buildPasteProposalInput,
  buildSelectionClipboardText,
  defaultParamsForDefinition,
  uniqueNodeId,
} from './graph-canvas-editing';

describe('graph canvas editing helpers', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('builds add-node inputs with default params and unique ids', () => {
    const input = buildAddNodeProposalInput({
      baseVersionId: 'ver_1',
      definition: videoDefinition(),
      existingNodeIds: ['video_text_to_video_abc123'],
      position: { x: 220, y: 120 },
      suffix: 'abc123',
    });

    expect(input.ops).toEqual([{
      op: 'spawn_node',
      id: 'video_text_to_video_abc123_2',
      node_type: 'video.text_to_video',
      title: 'Text To Video',
      params: { prompt: '', duration_sec: 1, aspect_ratio: '1:1' },
      pos: [220, 120],
    }]);
    expect(defaultParamsForDefinition(numberDefinition())).toEqual({ seed: 0 });
  });

  it('sizes a media card when adding image input', () => {
    const input = buildAddNodeProposalInput({
      baseVersionId: 'ver_1',
      definition: definition('input.image', 'Image Input', []),
      existingNodeIds: [],
      position: { x: 80, y: 80 },
      suffix: 'abc123',
    });

    expect(input.ops).toEqual([
      {
        op: 'spawn_node',
        id: 'input_image_abc123',
        node_type: 'input.image',
        title: 'Image Input',
        params: {},
        pos: [80, 80],
      },
      { op: 'resize_node', id: 'input_image_abc123', size: [280, 280] },
    ]);
  });

  it('fails closed when adding from + cannot wire a lineage edge', () => {
    expect(() => buildAddNodeProposalInput({
      baseVersionId: 'ver_1',
      definition: definition('input.image', 'Image Input', []),
      existingNodeIds: ['photo'],
      position: { x: 400, y: 40 },
      suffix: 'next',
      connectFrom: { nodeId: 'photo', definition: definition('input.image', 'Image Input', []) },
    })).toThrow('连线失败：这两张卡没有可接的端口');
  });

  it('wires a new image card to the source card', () => {
    const imageDef = definition('input.image', 'Image Input', []);
    imageDef.inputs = [{ name: 'in', type: 'IMAGE', required: false, cardinality: 'many' }];
    imageDef.outputs = [{ name: 'image', type: 'IMAGE', required: true }];
    const input = buildAddNodeProposalInput({
      baseVersionId: 'ver_1',
      definition: imageDef,
      existingNodeIds: ['photo'],
      position: { x: 400, y: 40 },
      suffix: 'next',
      connectFrom: { nodeId: 'photo', definition: imageDef },
    });
    expect(input.ops).toContainEqual({
      op: 'spawn_node',
      id: 'input_image_next',
      node_type: 'input.image',
      title: 'Image Input',
      params: {},
      pos: [400, 40],
      from: 'photo',
    });
    expect(input.ops.some((op) => op.op === 'add_edge')).toBe(false);
  });

  it('fans extra selected sources into the new card', () => {
    const imageDef = definition('input.image', 'Image Input', []);
    imageDef.inputs = [{ name: 'in', type: 'IMAGE', required: false, cardinality: 'many' }];
    imageDef.outputs = [{ name: 'image', type: 'IMAGE', required: true }];
    const input = buildAddNodeProposalInput({
      baseVersionId: 'ver_1',
      definition: imageDef,
      existingNodeIds: ['photo', 'still'],
      position: { x: 400, y: 40 },
      suffix: 'fan',
      connectFrom: {
        nodeId: 'photo',
        definition: imageDef,
        extra: [{ nodeId: 'still', definition: imageDef }],
      },
    });
    expect(input.ops).toContainEqual({
      op: 'spawn_node',
      id: 'input_image_fan',
      node_type: 'input.image',
      title: 'Image Input',
      params: {},
      pos: [400, 40],
      from: 'photo',
    });
    expect(input.ops).toContainEqual({
      op: 'add_edge',
      from: ['still', 'image'],
      to: ['input_image_fan', 'in'],
      edge_type: 'image',
    });
  });

  it('skips extra selected sources that cannot wire into the new card', () => {
    const imageDef = definition('input.image', 'Image Input', []);
    imageDef.inputs = [{ name: 'in', type: 'IMAGE', required: false, cardinality: 'many' }];
    imageDef.outputs = [{ name: 'image', type: 'IMAGE', required: true }];
    const seedDef = numberDefinition();
    const input = buildAddNodeProposalInput({
      baseVersionId: 'ver_1',
      definition: imageDef,
      existingNodeIds: ['photo', 'seed'],
      position: { x: 400, y: 40 },
      suffix: 'skip',
      connectFrom: {
        nodeId: 'photo',
        definition: imageDef,
        extra: [{ nodeId: 'seed', definition: seedDef }],
      },
    });
    expect(input.ops.some((op) => op.op === 'add_edge')).toBe(false);
  });

  it('fans in every selected source when adding from the dock', () => {
    const imageDef = definition('input.image', 'Image Input', []);
    imageDef.outputs = [{ name: 'image', type: 'IMAGE', required: true }];
    const connectFrom = fanInConnectFrom(
      ['photo', 'still'],
      [
        { id: 'photo', nodeType: 'input.image' },
        { id: 'still', nodeType: 'input.image' },
      ],
      new Map([['input.image', imageDef]]),
    );
    expect(connectFrom?.nodeId).toBe('photo');
    expect(connectFrom?.extra).toEqual([{ nodeId: 'still', definition: imageDef }]);
    expect(fanInConnectFrom(['photo'], [{ id: 'photo', nodeType: 'input.image' }], new Map([['input.image', imageDef]])))
      .toBeUndefined();
  });

  it('wires a new image card into the source when adding from the left handle', () => {
    const imageDef = definition('input.image', 'Image Input', []);
    imageDef.inputs = [{ name: 'in', type: 'IMAGE', required: false, cardinality: 'many' }];
    imageDef.outputs = [{ name: 'image', type: 'IMAGE', required: true }];
    const input = buildAddNodeProposalInput({
      baseVersionId: 'ver_1',
      definition: imageDef,
      existingNodeIds: ['photo'],
      position: { x: 40, y: 40 },
      suffix: 'prev',
      connectFrom: { nodeId: 'photo', definition: imageDef, direction: 'in' },
    });
    expect(input.ops).toContainEqual({
      op: 'add_edge',
      from: ['input_image_prev', 'image'],
      to: ['photo', 'in'],
      edge_type: 'image',
    });
  });

  it('duplicates a media card beside the source', () => {
    const input = buildDuplicateNodeProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: ['photo'],
      source: {
        id: 'photo',
        nodeType: 'input.image',
        title: '产品图',
        category: 'Input',
        status: 'queued',
        position: { x: 80, y: 40 },
        provider: null,
        summary: '{}',
        size: { width: 320, height: 240 },
      },
      params: { storage_uri: 'upload://abc' },
    });
    expect(input.ops).toEqual([
      {
        op: 'spawn_node',
        id: 'input_image',
        node_type: 'input.image',
        title: '产品图 copy',
        params: { storage_uri: 'upload://abc' },
        pos: [110, 70],
        from: 'photo',
      },
      { op: 'resize_node', id: 'input_image', size: [320, 240] },
    ]);
  });

  it('places text-to-image beside an empty media card', () => {
    const input = buildMediaGenerateProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: ['photo'],
      sourceNodeId: 'photo',
      sourceTitle: '图片',
      sourceX: 80,
      sourceY: 40,
      sourceWidth: 320,
      prompt: 'product hero',
      aspectRatio: '16:9',
      hasImage: false,
    });
    expect(input.ops).toEqual([
      {
        op: 'spawn_node',
        id: 'image_generate',
        node_type: 'image.generate',
        title: '图片',
        params: { prompt: 'product hero', aspect_ratio: '16:9' },
        pos: [448, 40],
        from: 'photo',
      },
      { op: 'resize_node', id: 'image_generate', size: [280, 280] },
    ]);
  });

  it('wires a filled media card into the new edit node', () => {
    const input = buildMediaGenerateProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: ['photo'],
      sourceNodeId: 'photo',
      sourceTitle: '产品图',
      sourceX: 80,
      sourceY: 40,
      sourceWidth: 320,
      prompt: 'make it night',
      aspectRatio: '1:1',
      hasImage: true,
    });
    expect(input.ops).toEqual([
      {
        op: 'spawn_node',
        id: 'image_edit',
        node_type: 'image.edit',
        title: '产品图 生成',
        params: { prompt: 'make it night' },
        pos: [448, 40],
        from: 'photo',
      },
      { op: 'resize_node', id: 'image_edit', size: [280, 280] },
    ]);
  });

  it('pins catalog semantics on the generate node when a model was chosen', () => {
    const input = buildMediaGenerateProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: ['photo'],
      sourceNodeId: 'photo',
      sourceTitle: '图片',
      sourceX: 80,
      sourceY: 40,
      sourceWidth: 280,
      prompt: 'product hero',
      aspectRatio: '1:1',
      hasImage: false,
      semantics: {
        capabilityId: 'text_to_image',
        mode: 'text_to_image',
        implementation: {
          pinned: { requestedModelId: 'google/nano-banana-2', bindingId: 'nano.atlas.v1' },
        },
      },
    });
    expect(input.ops).toContainEqual({
      op: 'set_semantics',
      id: 'image_generate',
      semantics: {
        capabilityId: 'text_to_image',
        mode: 'text_to_image',
        implementation: {
          pinned: { requestedModelId: 'google/nano-banana-2', bindingId: 'nano.atlas.v1' },
        },
      },
    });
  });

  it('spawns a video generate card without wiring an incompatible lineage edge', () => {
    expect(mediaGenerateTarget({ nodeType: 'input.video', hasImage: false })).toEqual({
      capabilityId: 'text_to_video',
      mediaKind: 'video',
      nodeType: 'video.text_to_video',
      idBase: 'video_text_to_video',
      connectFrom: false,
    });
    expect(mediaGenerateTarget({ nodeType: 'input.audio', hasImage: false })).toBeNull();
    expect(mediaGenerateTarget({ nodeType: 'input.text', hasImage: false })).toEqual({
      capabilityId: 'text_to_image',
      mediaKind: 'image',
      nodeType: 'image.generate',
      idBase: 'image_generate',
      connectFrom: true,
    });
    expect(mediaGenerateTarget({ nodeType: 'input.text', hasImage: false, mediaKind: 'video' })).toEqual({
      capabilityId: 'text_to_video',
      mediaKind: 'video',
      nodeType: 'video.text_to_video',
      idBase: 'video_text_to_video',
      connectFrom: true,
    });
    expect(mediaGenerateTarget({ nodeType: 'output.save', hasImage: true })).toBeNull();
    const input = buildMediaGenerateProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: ['clip'],
      sourceNodeId: 'clip',
      sourceTitle: '视频',
      sourceX: 80,
      sourceY: 40,
      sourceWidth: 320,
      prompt: 'product video',
      aspectRatio: '16:9',
      hasImage: false,
      sourceNodeType: 'input.video',
    });
    expect(input.ops).toEqual([
      {
        op: 'spawn_node',
        id: 'video_text_to_video',
        node_type: 'video.text_to_video',
        title: '视频',
        params: { prompt: 'product video', aspect_ratio: '16:9', duration_sec: 5 },
        pos: [448, 40],
      },
      { op: 'resize_node', id: 'video_text_to_video', size: [280, 280] },
    ]);
  });

  it('stacks multiple generate cards and keeps the chosen video duration', () => {
    const input = buildMediaGenerateProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: ['copy'],
      sourceNodeId: 'copy',
      sourceTitle: '文案',
      sourceX: 40,
      sourceY: 80,
      sourceWidth: 280,
      prompt: 'product video',
      aspectRatio: '16:9',
      hasImage: false,
      sourceNodeType: 'input.text',
      mediaKind: 'video',
      durationSec: 8,
      count: 4,
    });
    expect(input.ops.filter((op) => op.op === 'spawn_node')).toEqual([
      {
        op: 'spawn_node',
        id: 'video_text_to_video',
        node_type: 'video.text_to_video',
        title: '文案',
        params: { prompt: 'product video', aspect_ratio: '16:9', duration_sec: 8 },
        pos: [368, 80],
        from: 'copy',
      },
      {
        op: 'spawn_node',
        id: 'video_text_to_video_2',
        node_type: 'video.text_to_video',
        title: '文案',
        params: { prompt: 'product video', aspect_ratio: '16:9', duration_sec: 8 },
        pos: [368, 104],
        from: 'copy',
      },
      {
        op: 'spawn_node',
        id: 'video_text_to_video_3',
        node_type: 'video.text_to_video',
        title: '文案',
        params: { prompt: 'product video', aspect_ratio: '16:9', duration_sec: 8 },
        pos: [368, 128],
        from: 'copy',
      },
      {
        op: 'spawn_node',
        id: 'video_text_to_video_4',
        node_type: 'video.text_to_video',
        title: '文案',
        params: { prompt: 'product video', aspect_ratio: '16:9', duration_sec: 8 },
        pos: [368, 152],
        from: 'copy',
      },
    ]);
  });

  it('extracts a video frame as a disconnected image card', () => {
    expect(videoFrameLabel('first')).toBe('首帧');
    const input = buildVideoFrameProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: ['clip'],
      sourceNodeId: 'clip',
      sourceTitle: '成片',
      sourceX: 80,
      sourceY: 40,
      sourceWidth: 320,
      storageUri: 'upload://frame',
      size: { width: 360, height: 203 },
      kind: 'last',
    });
    expect(input.ops).toEqual([
      {
        op: 'spawn_node',
        id: 'image_frame_last',
        node_type: 'input.image',
        title: '成片 末帧',
        params: { storage_uri: 'upload://frame' },
        pos: [424, 40],
      },
      { op: 'resize_node', id: 'image_frame_last', size: [360, 203] },
    ]);
    expect(input.ops.some((op) => 'from' in op && op.from)).toBe(false);
  });

  it('adds 宫格切片 as input.image nodes without removing the source', () => {
    const input = buildGridSplitProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: ['photo'],
      sourceNodeId: 'photo',
      sourceTitle: '产品主图',
      placements: [
        { row: 0, column: 0, x: 500, y: 40 },
        { row: 0, column: 1, x: 760, y: 40 },
      ],
      tiles: [
        { storageUri: 'upload://a', row: 0, column: 0, size: { width: 280, height: 280 } },
        { storageUri: 'upload://b', row: 0, column: 1, size: { width: 280, height: 280 } },
      ],
    });
    expect(input.label).toBe('宫格切分 2 张');
    expect(input.ops).toEqual([
      {
        op: 'spawn_node',
        id: 'image_grid_r1c1',
        node_type: 'input.image',
        title: '产品主图 r1c1',
        params: { storage_uri: 'upload://a' },
        pos: [500, 40],
        from: 'photo',
      },
      { op: 'resize_node', id: 'image_grid_r1c1', size: [280, 280] },
      {
        op: 'spawn_node',
        id: 'image_grid_r1c2',
        node_type: 'input.image',
        title: '产品主图 r1c2',
        params: { storage_uri: 'upload://b' },
        pos: [760, 40],
        from: 'photo',
      },
      { op: 'resize_node', id: 'image_grid_r1c2', size: [280, 280] },
    ]);
  });

  it('imports dropped files as media cards without removing anything', () => {
    const input = buildMediaIngestProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: [],
      items: [
        {
          nodeType: 'input.image',
          title: 'poster.png',
          storageUri: 'upload://img_1',
          position: { x: 40, y: 80 },
          size: { width: 320, height: 200 },
        },
      ],
    });
    expect(input.label).toBe('导入 1 个素材');
    expect(input.ops).toEqual([
      {
        op: 'spawn_node',
        id: 'input_image',
        node_type: 'input.image',
        title: 'poster.png',
        params: { storage_uri: 'upload://img_1' },
        pos: [40, 80],
      },
      { op: 'resize_node', id: 'input_image', size: [320, 200] },
    ]);
  });

  it('places a pixel crop to the right of the source and keeps the source', () => {
    const input = buildCropResultProposalInput({
      baseVersionId: 'ver_1',
      existingNodeIds: ['photo'],
      sourceNodeId: 'photo',
      sourceTitle: '产品主图',
      sourceX: 40,
      sourceY: 80,
      sourceWidth: 240,
      storageUri: 'upload://crop_1',
      size: { width: 200, height: 180 },
    });
    expect(input.ops[0]).toMatchObject({
      op: 'spawn_node',
      node_type: 'input.image',
      title: '产品主图 裁剪',
      params: { storage_uri: 'upload://crop_1' },
      pos: [304, 80],
      from: 'photo',
    });
    expect(input.ops.some((op) => op.op === 'remove_node')).toBe(false);
  });

  it('deletes selected nodes as one batch and lets GraphService remove edges', () => {
    expect(
      buildDeleteNodesProposalInput({
        baseVersionId: 'ver_1',
        selectedNodeIds: ['video', 'text'],
      })?.ops,
    ).toEqual([
      { op: 'remove_node', id: 'text' },
      { op: 'remove_node', id: 'video' },
    ]);
  });

  it('copies and pastes an internal subgraph with rewritten ids', () => {
    const text = buildSelectionClipboardText({
      sourceVersionId: 'ver_1',
      nodes: [textNode(), videoNode()],
      edges: [textToVideoEdge()],
      workflowGraph: workflowGraph(),
    });
    const input = buildPasteProposalInput({
      baseVersionId: 'ver_2',
      existingNodeIds: ['text', 'video'],
      text,
      position: { x: 500, y: 300 },
    });

    expect(input?.ops).toEqual([
      {
        op: 'spawn_node',
        id: 'text_2',
        node_type: 'input.text',
        title: 'Text input',
        params: { text: 'hello' },
        pos: [500, 300],
      },
      {
        op: 'spawn_node',
        id: 'video_2',
        node_type: 'video.text_to_video',
        title: 'Video render',
        params: { prompt: '', duration_sec: 4, aspect_ratio: '1:1' },
        pos: [780, 300],
      },
      {
        op: 'add_edge',
        from: ['text_2', 'text'],
        to: ['video_2', 'prompt'],
        edge_type: 'text',
      },
    ]);
    expect(buildPasteProposalInput({
      baseVersionId: 'ver_2',
      existingNodeIds: [],
      text: '{"kind":"wrong"}',
      position: { x: 0, y: 0 },
    })).toBeNull();
  });

  it('generates deterministic unique ids from existing ids', () => {
    expect(uniqueNodeId(['node', 'node_2'], 'node')).toBe('node_3');
  });

  it('places repeated library additions in the nearest non-overlapping slot', () => {
    const first = node('first', 'input.text', 'First', 'Input', 400, 300);
    const second = node('second', 'input.text', 'Second', 'Input', 136, 160);

    const position = nextAvailableNodePosition({ x: 400, y: 300 }, [first, second]);

    expect(position).not.toEqual({ x: 400, y: 300 });
    expect(position).toEqual({ x: 136, y: 440 });
  });

  it('does not dispatch add, delete, or paste mutations in view mode', async () => {
    const onCreateProposal = vi.fn(async () => undefined);
    const setClipboardStatus = vi.fn();
    const readText = vi.fn(async () => buildSelectionClipboardText({
      sourceVersionId: 'ver_1',
      nodes: [textNode()],
      edges: [],
      workflowGraph: workflowGraph(),
    }));
    vi.stubGlobal('navigator', { clipboard: { readText } });
    const actions = createCanvasEditActions({
      capabilities: canvasCapabilities('view', true),
      drawGraph: { nodes: [textNode(), videoNode()], edges: [textToVideoEdge()] },
      onCreateProposal,
      setClipboardStatus,
      versionId: 'ver_1',
      view: { x: 0, y: 0, z: 1 },
      viewportSize: { width: 800, height: 600 },
      workflowGraph: workflowGraph(),
    });

    actions.addNode(videoDefinition());
    actions.duplicateNode(textNode());
    actions.deleteSelection(new Set(['video']));
    await actions.pasteSelection();

    expect(onCreateProposal).not.toHaveBeenCalled();
    expect(readText).not.toHaveBeenCalled();
    expect(setClipboardStatus.mock.calls.map(([message]) => message)).toEqual([
      '当前模式不允许添加节点',
      '当前模式不允许添加节点',
      '当前模式不允许删除',
      '当前模式不允许粘贴',
    ]);
  });

  it('places library and paste nodes at the last canvas pointer', async () => {
    const onCreateProposal = vi.fn<(input: ManualProposalInput) => Promise<void>>(async () => undefined);
    vi.stubGlobal('navigator', {
      clipboard: {
        readText: vi.fn(async () => buildSelectionClipboardText({
          sourceVersionId: 'ver_1',
          nodes: [textNode()],
          edges: [],
          workflowGraph: workflowGraph(),
        })),
      },
    });
    const actions = createCanvasEditActions({
      capabilities: canvasCapabilities('edit', true),
      drawGraph: { nodes: [textNode()], edges: [] },
      onCreateProposal,
      pointerWorld: () => ({ x: 640, y: 220 }),
      setClipboardStatus: vi.fn(),
      versionId: 'ver_1',
      view: { x: 0, y: 0, z: 1 },
      viewportSize: { width: 800, height: 600 },
      workflowGraph: workflowGraph(),
    });

    actions.addNode(definition('input.image', 'Image Input', []));
    await actions.pasteSelection();

    expect(onCreateProposal.mock.calls.map((call) => call[0]?.ops[0])).toEqual([
      expect.objectContaining({
        op: 'spawn_node',
        node_type: 'input.image',
        pos: [640, 220],
      }),
      expect.objectContaining({
        op: 'spawn_node',
        pos: [640, 220],
      }),
    ]);
  });
});

describe('derived media cards always inherit a lineage edge', () => {
  const source = {
    baseVersionId: 'ver_1',
    existingNodeIds: ['photo'],
    sourceNodeId: 'photo',
    sourceTitle: '产品图',
    sourceX: 80,
    sourceY: 40,
    sourceWidth: 280,
  } as const;

  it.each([
    [
      'empty generate',
      buildMediaGenerateProposalInput({
        ...source,
        prompt: 'hero',
        aspectRatio: '1:1',
        hasImage: false,
      }).ops,
    ],
    [
      'filled generate',
      buildMediaGenerateProposalInput({
        ...source,
        prompt: 'hero',
        aspectRatio: '1:1',
        hasImage: true,
      }).ops,
    ],
    [
      'crop',
      buildCropResultProposalInput({
        ...source,
        storageUri: 'upload://crop',
        size: { width: 200, height: 180 },
      }).ops,
    ],
    [
      'grid split',
      buildGridSplitProposalInput({
        ...source,
        placements: [
          { row: 0, column: 0, x: 400, y: 40 },
          { row: 0, column: 1, x: 680, y: 40 },
        ],
        tiles: [
          { storageUri: 'upload://a', row: 0, column: 0 },
          { storageUri: 'upload://b', row: 0, column: 1 },
        ],
      }).ops,
    ],
    [
      'outpaint',
      buildImageCanvasToolResultProposalInput({
        ...source,
        kind: 'outpaint',
        storageUri: 'upload://outpaint',
        size: { width: 200, height: 180 },
      }).ops,
    ],
  ] satisfies Array<[string, ManualProposalInput['ops']]>)(
    '%s wires every new card back to the source',
    (_name, ops) => {
      const spawned = ops.filter((op) => op.op === 'spawn_node');
      expect(spawned.length).toBeGreaterThan(0);
      for (const node of spawned) {
        if (node.op !== 'spawn_node') continue;
        expect(node.from).toBe('photo');
      }
    },
  );
});

function textNode(): WorkbenchState['graph']['nodes'][number] {
  return node('text', 'input.text', 'Text input', 'Input', 40, 80);
}

function videoNode(): WorkbenchState['graph']['nodes'][number] {
  return node('video', 'video.text_to_video', 'Video render', 'Video', 320, 80);
}

function node(
  id: string,
  nodeType: string,
  title: string,
  category: string,
  x: number,
  y: number,
): WorkbenchState['graph']['nodes'][number] {
  return {
    id,
    nodeType,
    title,
    category,
    status: 'queued',
    position: { x, y },
    provider: null,
    summary: '{}',
  };
}

function textToVideoEdge(): WorkbenchState['graph']['edges'][number] {
  return {
    id: 'edge_text_video',
    from: { nodeId: 'text', port: 'text' },
    to: { nodeId: 'video', port: 'prompt' },
    kind: 'text',
  };
}

function workflowGraph(): WorkflowGraph {
  return {
    schema_version: 1,
    nodes: {
      text: {
        node_type: 'input.text',
        title: 'Text input',
        params: { text: 'hello' },
        pos: [40, 80],
      },
      video: {
        node_type: 'video.text_to_video',
        title: 'Video render',
        params: { prompt: '', duration_sec: 4, aspect_ratio: '1:1' },
        pos: [320, 80],
      },
    },
    edges: [{ from: ['text', 'text'], to: ['video', 'prompt'], edge_type: 'text' }],
  };
}

function videoDefinition(): NodeDefinition {
  return definition(
    'video.text_to_video',
    'Text To Video',
    [
      ['prompt', { type: 'string', enum_values: [], minimum: null, maximum: null }],
      ['duration_sec', { type: 'integer', enum_values: [], minimum: 1, maximum: 10 }],
      ['aspect_ratio', { type: 'string', enum_values: ['1:1', '9:16'], minimum: null, maximum: null }],
    ],
  );
}

function numberDefinition(): NodeDefinition {
  return definition('mock.seed', 'Seed', [
    ['seed', { type: 'integer', enum_values: [], minimum: null, maximum: null }],
  ]);
}

function definition(
  type: string,
  title: string,
  properties: Array<[string, NodeDefinition['params_schema']['properties'][string]]>,
): NodeDefinition {
  return {
    type,
    title,
    category: type.split('.')[0] ?? 'node',
    provider: null,
    capability: null,
    description: title,
    inputs: [],
    outputs: [],
    params_schema: {
      required: properties.map(([key]) => key),
      properties: Object.fromEntries(properties),
      allow_unknown: false,
    },
    estimated_cost: null,
  };
}
