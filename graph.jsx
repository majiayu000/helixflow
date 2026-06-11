// graph.jsx — a realistic SDXL text-to-image ComfyUI node graph, reused across variations.

const HEAD = 31, PAD = 8, ROW = 26;
const portY = (n, r) => n.y + HEAD + PAD + r * ROW + ROW / 2;
const rX = (n) => n.x + (n.w || 188);
const lX = (n) => n.x;

// node definitions for a clean SDXL T2I workflow
function t2iNodes(extra = {}) {
  const N = {
    ckpt:   { x: 16,  y: 64,  title: 'Load Checkpoint', swatch: 'var(--t-model)',
              inputs: [], outputs: [{ label: 'MODEL', type: 'MODEL' }, { label: 'CLIP', type: 'CLIP' }, { label: 'VAE', type: 'VAE' }],
              params: [{ k: 'ckpt', v: 'sdxl_base.safetensors' }] },
    pos:    { x: 244, y: 40,  title: 'CLIP Text Encode', swatch: 'var(--t-cond)',
              inputs: [{ label: 'clip', type: 'CLIP' }], outputs: [{ label: 'COND', type: 'CONDITIONING' }],
              params: [{ k: 'text', v: '"a product shot…"' }] },
    neg:    { x: 244, y: 184, title: 'CLIP Text Encode', swatch: 'var(--t-cond)',
              inputs: [{ label: 'clip', type: 'CLIP' }], outputs: [{ label: 'COND', type: 'CONDITIONING' }],
              params: [{ k: 'text', v: '"blurry, low…"' }] },
    latent: { x: 244, y: 328, title: 'Empty Latent', swatch: 'var(--t-latent)',
              inputs: [], outputs: [{ label: 'LATENT', type: 'LATENT' }],
              params: [{ k: 'w × h', v: '1024 × 1024' }, { k: 'batch', v: '1' }] },
    ks:     { x: 480, y: 84,  title: 'KSampler', swatch: 'var(--t-model)',
              inputs: [{ label: 'model', type: 'MODEL' }, { label: 'positive', type: 'CONDITIONING' }, { label: 'negative', type: 'CONDITIONING' }, { label: 'latent', type: 'LATENT' }],
              outputs: [{ label: 'LATENT', type: 'LATENT' }],
              params: [{ k: 'seed', v: '843…21' }, { k: 'steps', v: '28' }, { k: 'cfg', v: '6.5' }] },
    vae:    { x: 712, y: 116, title: 'VAE Decode', swatch: 'var(--t-vae)',
              inputs: [{ label: 'samples', type: 'LATENT' }, { label: 'vae', type: 'VAE' }],
              outputs: [{ label: 'IMAGE', type: 'IMAGE' }], params: [] },
    save:   { x: 916, y: 144, title: 'Save Image', swatch: 'var(--t-image)',
              inputs: [{ label: 'images', type: 'IMAGE' }], outputs: [], params: [], thumb: true },
  };
  for (const k in extra) Object.assign(N[k], extra[k]);
  return N;
}

const t2iEdges = (N) => [
  { x1: rX(N.ckpt), y1: portY(N.ckpt, 0), x2: lX(N.ks), y2: portY(N.ks, 0), c: 'var(--t-model)' },
  { x1: rX(N.ckpt), y1: portY(N.ckpt, 1), x2: lX(N.pos), y2: portY(N.pos, 0), c: 'var(--t-clip)' },
  { x1: rX(N.ckpt), y1: portY(N.ckpt, 1), x2: lX(N.neg), y2: portY(N.neg, 0), c: 'var(--t-clip)' },
  { x1: rX(N.ckpt), y1: portY(N.ckpt, 2), x2: lX(N.vae), y2: portY(N.vae, 1), c: 'var(--t-vae)' },
  { x1: rX(N.pos), y1: portY(N.pos, 0), x2: lX(N.ks), y2: portY(N.ks, 1), c: 'var(--t-cond)' },
  { x1: rX(N.neg), y1: portY(N.neg, 0), x2: lX(N.ks), y2: portY(N.ks, 2), c: 'var(--t-cond)' },
  { x1: rX(N.latent), y1: portY(N.latent, 0), x2: lX(N.ks), y2: portY(N.ks, 3), c: 'var(--t-latent)' },
  { x1: rX(N.ks), y1: portY(N.ks, 0), x2: lX(N.vae), y2: portY(N.vae, 0), c: 'var(--t-latent)' },
  { x1: rX(N.vae), y1: portY(N.vae, 0), x2: lX(N.save), y2: portY(N.save, 0), c: 'var(--t-image)' },
];

// renders the graph at a given scale inside an absolutely-positioned layer
function T2IGraph({ scale = 1, offset = { x: 0, y: 0 }, nodes, edges, edgeWidth = 2, fade = false }) {
  const N = nodes || t2iNodes();
  const E = edges || t2iEdges(N);
  return (
    <div style={{ position: 'absolute', left: offset.x, top: offset.y, transform: `scale(${scale})`, transformOrigin: 'top left' }}>
      <EdgeLayer>
        {E.map((e, i) => <Edge key={i} {...e} color={e.c} w={edgeWidth} dashed={e.dashed} />)}
      </EdgeLayer>
      {Object.values(N).map((n, i) => <NodeCard key={i} {...n} dim={fade && n.dim} />)}
    </div>
  );
}

Object.assign(window, { t2iNodes, t2iEdges, T2IGraph, portY, rX, lX });
