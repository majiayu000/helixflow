// proto-data.jsx — graph versions, proposals, run scripts for the interactive prototype.
// Edges reference [nodeKey, rowIndex] so coordinates are computed at render time.

// ---------- node definitions ----------
const PD_NODES = {
  ckpt:   { x: 30,  y: 70,  title: 'Load Checkpoint', swatch: 'var(--t-model)',
            inputs: [], outputs: [{ label: 'MODEL', type: 'MODEL' }, { label: 'CLIP', type: 'CLIP' }, { label: 'VAE', type: 'VAE' }],
            params: [{ k: 'ckpt', v: 'sdxl_base.safetensors' }] },
  pos:    { x: 268, y: 40,  title: 'CLIP Text Encode', swatch: 'var(--t-cond)',
            inputs: [{ label: 'clip', type: 'CLIP' }], outputs: [{ label: 'COND', type: 'CONDITIONING' }],
            params: [{ k: 'text', v: '"产品摄影棚拍…"' }] },
  neg:    { x: 268, y: 192, title: 'CLIP Text Encode', swatch: 'var(--t-cond)',
            inputs: [{ label: 'clip', type: 'CLIP' }], outputs: [{ label: 'COND', type: 'CONDITIONING' }],
            params: [{ k: 'text', v: '"模糊, 低质量…"' }] },
  latent: { x: 268, y: 344, title: 'Empty Latent', swatch: 'var(--t-latent)',
            inputs: [], outputs: [{ label: 'LATENT', type: 'LATENT' }],
            params: [{ k: 'w × h', v: '1024 × 1024' }, { k: 'batch', v: '1' }] },
  ks:     { x: 520, y: 86,  title: 'KSampler', swatch: 'var(--t-model)',
            inputs: [{ label: 'model', type: 'MODEL' }, { label: 'positive', type: 'CONDITIONING' }, { label: 'negative', type: 'CONDITIONING' }, { label: 'latent', type: 'LATENT' }],
            outputs: [{ label: 'LATENT', type: 'LATENT' }],
            params: [{ k: 'seed', v: '843629104821' }, { k: 'steps', v: '28' }, { k: 'cfg', v: '6.5' }] },
  vae:    { x: 768, y: 118, title: 'VAE Decode', swatch: 'var(--t-vae)',
            inputs: [{ label: 'samples', type: 'LATENT' }, { label: 'vae', type: 'VAE' }],
            outputs: [{ label: 'IMAGE', type: 'IMAGE' }], params: [] },
  save:   { x: 988, y: 146, title: 'Save Image', swatch: 'var(--t-image)',
            inputs: [{ label: 'images', type: 'IMAGE' }], outputs: [], params: [], thumb: true },
  // --- ControlNet depth branch (v2) ---
  img:    { x: 30,  y: 430, title: 'Load Image', swatch: 'var(--t-image)',
            inputs: [], outputs: [{ label: 'IMAGE', type: 'IMAGE' }, { label: 'MASK', type: 'MASK' }],
            params: [{ k: 'file', v: 'ref_depth.png' }] },
  depth:  { x: 268, y: 488, title: 'Depth Anything', swatch: 'var(--t-image)',
            inputs: [{ label: 'image', type: 'IMAGE' }], outputs: [{ label: 'IMAGE', type: 'IMAGE' }],
            params: [{ k: 'resolution', v: '1024' }] },
  cnl:    { x: 30,  y: 612, title: 'Load ControlNet', swatch: 'var(--t-model)',
            inputs: [], outputs: [{ label: 'CONTROL_NET', type: 'CONTROL_NET' }],
            params: [{ k: 'model', v: 'control-lora-depth.sft' }] },
  cna:    { x: 520, y: 392, title: 'Apply ControlNet', swatch: 'var(--t-cond)',
            inputs: [{ label: 'cond', type: 'CONDITIONING' }, { label: 'control_net', type: 'CONTROL_NET' }, { label: 'image', type: 'IMAGE' }],
            outputs: [{ label: 'COND', type: 'CONDITIONING' }],
            params: [{ k: 'strength', v: '0.85' }] },
};
const PD_ID = { ckpt: '#1', pos: '#2', neg: '#3', latent: '#4', ks: '#5', vae: '#6', save: '#7', img: '#8', depth: '#9', cnl: '#10', cna: '#11' };

// ---------- edges as [fromKey,row] -> [toKey,row] ----------
const PD_EDGES_V1 = [
  { f: ['ckpt', 0], t: ['ks', 0], type: 'MODEL' },
  { f: ['ckpt', 1], t: ['pos', 0], type: 'CLIP' },
  { f: ['ckpt', 1], t: ['neg', 0], type: 'CLIP' },
  { f: ['ckpt', 2], t: ['vae', 1], type: 'VAE' },
  { f: ['pos', 0], t: ['ks', 1], type: 'CONDITIONING', id: 'pos-ks' },
  { f: ['neg', 0], t: ['ks', 2], type: 'CONDITIONING' },
  { f: ['latent', 0], t: ['ks', 3], type: 'LATENT' },
  { f: ['ks', 0], t: ['vae', 0], type: 'LATENT' },
  { f: ['vae', 0], t: ['save', 0], type: 'IMAGE' },
];
const PD_EDGES_CN = [
  { f: ['img', 0], t: ['depth', 0], type: 'IMAGE' },
  { f: ['pos', 0], t: ['cna', 0], type: 'CONDITIONING' },
  { f: ['cnl', 0], t: ['cna', 1], type: 'CONTROL_NET' },
  { f: ['depth', 0], t: ['cna', 2], type: 'IMAGE' },
  { f: ['cna', 0], t: ['ks', 1], type: 'CONDITIONING' },
];
const PD_EDGES_V2 = [...PD_EDGES_V1.filter(e => e.id !== 'pos-ks'), ...PD_EDGES_CN];

const V1_KEYS = ['ckpt', 'pos', 'neg', 'latent', 'ks', 'vae', 'save'];
const V2_KEYS = [...V1_KEYS, 'img', 'depth', 'cnl', 'cna'];

function pickNodes(keys, overrides = {}) {
  const out = {};
  keys.forEach(k => { out[k] = { ...PD_NODES[k], params: PD_NODES[k].params.map(p => ({ ...p })) }; });
  for (const k in overrides) Object.assign(out[k], overrides[k]);
  return out;
}

// graph snapshots per version
const GRAPH_EMPTY = { nodes: {}, edges: [] };
const GRAPH_V1 = { nodes: pickNodes(V1_KEYS), edges: PD_EDGES_V1 };
const GRAPH_V2 = { nodes: pickNodes(V2_KEYS), edges: PD_EDGES_V2 };
const GRAPH_V3 = { nodes: pickNodes(V2_KEYS), edges: PD_EDGES_V2 };
GRAPH_V3.nodes.cnl.params = [{ k: 'model', v: 'cn-depth-sdxl-1.0.sft' }];

// ---------- proposals ----------
const PROPOSAL_1 = {
  id: 'p1', kind: 'create', title: 'SDXL 文生图工作流',
  summary: '新建 7 个节点，连成标准文生图链路：Checkpoint → 双 CLIP 编码 → KSampler → VAE Decode → Save。分辨率 1024×1024，带负面 prompt。',
  changes: [
    { kind: 'add', lbl: 'Load Checkpoint', meta: '#1' },
    { kind: 'add', lbl: 'CLIP Text Encode ×2（正/负）', meta: '#2,3' },
    { kind: 'add', lbl: 'Empty Latent 1024×1024', meta: '#4' },
    { kind: 'add', lbl: 'KSampler（28 步 · cfg 6.5）', meta: '#5' },
    { kind: 'add', lbl: 'VAE Decode → Save Image', meta: '#6,7' },
  ],
  graph: GRAPH_V1, addedKeys: V1_KEYS, updatedKeys: [], versionLabel: 'SDXL 文生图',
};
const PROPOSAL_2 = {
  id: 'p2', kind: 'modify', title: '追加 ControlNet Depth 分支',
  summary: '基于参考图 ref_depth.png 增加深度控制：Load Image → Depth Anything → Apply ControlNet，插入到正向 conditioning 与 KSampler 之间。原节点全部保留。',
  changes: [
    { kind: 'add', lbl: 'Load Image（ref_depth.png）', meta: '#8' },
    { kind: 'add', lbl: 'Depth Anything 预处理', meta: '#9' },
    { kind: 'add', lbl: 'Load ControlNet', meta: '#10' },
    { kind: 'add', lbl: 'Apply ControlNet（strength 0.85）', meta: '#11' },
    { kind: 'del', lbl: '断开 CLIP正向 → KSampler', meta: 'edge' },
  ],
  graph: GRAPH_V2, addedKeys: ['img', 'depth', 'cnl', 'cna'], updatedKeys: [], versionLabel: '+ ControlNet Depth',
};
const PROPOSAL_FIX = {
  id: 'pfix', kind: 'fix', title: '最小修复：替换缺失的 ControlNet 模型',
  summary: '报错原因：models/controlnet 下不存在 control-lora-depth.sft。检测到可用的 cn-depth-sdxl-1.0.sft，仅替换 Load ControlNet 的 model 参数，其余不动。',
  changes: [
    { kind: 'upd', lbl: 'Load ControlNet · model 参数', meta: '#10' },
  ],
  graph: GRAPH_V3, addedKeys: [], updatedKeys: ['cnl'], versionLabel: '修复 ControlNet 模型',
};

// ---------- run scripts ----------
const RUN_V1 = { order: ['ckpt', 'pos', 'neg', 'latent', 'ks', 'vae', 'save'], dur: [450, 280, 280, 220, 2100, 480, 380], fail: null };
const RUN_V2_FAIL = {
  order: ['ckpt', 'pos', 'neg', 'img', 'depth', 'cnl'], dur: [400, 260, 260, 350, 700, 500],
  fail: { key: 'cnl', title: 'Load ControlNet 执行失败', raw: "FileNotFoundError: model 'control-lora-depth.sft' not found in models/controlnet", human: '找不到模型文件 control-lora-depth.sft —— 该文件不在本地 models/controlnet 目录中。' },
};
const RUN_V3 = { order: ['ckpt', 'pos', 'neg', 'latent', 'img', 'depth', 'cnl', 'cna', 'ks', 'vae', 'save'], dur: [380, 220, 220, 180, 300, 550, 320, 380, 1900, 420, 350], fail: null };

const SWEEP_SEEDS = ['102 334', '768 410', '843 629', '551 207'];
const SWEEP_HUES = [212, 28, 268, 152];

const AGENT_STATUSES = ['正在启动 Codex CLI…', '正在读取画布与 312 个节点定义…', '正在规划图变更…'];

Object.assign(window, {
  PD_NODES, PD_ID, PD_EDGES_V1, PD_EDGES_V2, GRAPH_EMPTY, GRAPH_V1, GRAPH_V2, GRAPH_V3,
  PROPOSAL_1, PROPOSAL_2, PROPOSAL_FIX, RUN_V1, RUN_V2_FAIL, RUN_V3,
  SWEEP_SEEDS, SWEEP_HUES, AGENT_STATUSES, V1_KEYS, V2_KEYS,
});
