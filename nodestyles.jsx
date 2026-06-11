// nodestyles.jsx — three visual styles for the node canvas.

function CanvasFrame({ cls = '', children }) {
  return (
    <div className={`wb-canvas ${cls}`} style={{ position: 'relative', width: '100%', height: '100%', borderRadius: 'inherit', overflow: 'hidden' }}>
      <div className="canvas-grid" />
      {children}
      <div className="zoom-ctl"><button>−</button><span>70%</span><button>＋</button></div>
    </div>
  );
}

// 1 · refined default (white nodes, type ports, soft bezier, dotted grid)
function CanvasRefined() {
  return (
    <CanvasFrame>
      <div className="canvas-toolbar"><span className="canvas-pill"><Ic n="layers" s={13} c="var(--text-3)" /> 标准 · 端口类型着色</span></div>
      <T2IGraph scale={0.7} offset={{ x: 24, y: 44 }} edgeWidth={2} />
    </CanvasFrame>
  );
}

// 2 · bold (colored headers + group frame + thick saturated links)
function CanvasBold() {
  const N = t2iNodes();
  return (
    <CanvasFrame cls="cv-bold">
      <div className="canvas-toolbar"><span className="canvas-pill"><Ic n="grid" s={13} c="var(--text-3)" /> 强对比 · 彩色头 + 分组</span></div>
      <div style={{ position: 'absolute', left: 24, top: 44, transform: 'scale(0.7)', transformOrigin: 'top left' }}>
        {/* group frame behind the two prompt nodes */}
        <div className="cv-group" style={{ left: N.pos.x - 14, top: N.pos.y - 16, width: 216, height: 290 }}>
          <span className="cv-group-label">Conditioning</span>
        </div>
        <EdgeLayer>{t2iEdges(N).map((e, i) => <Edge key={i} {...e} color={e.c} w={3} />)}</EdgeLayer>
        {Object.values(N).map((n, i) => <NodeCard key={i} {...n} />)}
      </div>
    </CanvasFrame>
  );
}

// 3 · flat (wireframe / diagram, line grid, left-spine headers)
function CanvasFlat() {
  const N = t2iNodes();
  // trim params for a cleaner diagram look
  ['ckpt', 'pos', 'neg', 'latent', 'ks', 'vae'].forEach(k => { N[k].params = N[k].params.slice(0, 1); });
  N.save.thumb = false;
  return (
    <CanvasFrame cls="cv-flat">
      <div className="canvas-toolbar"><span className="canvas-pill"><Ic n="layers" s={13} c="var(--text-3)" /> 极简 · 线框图</span></div>
      <div style={{ position: 'absolute', left: 24, top: 44, transform: 'scale(0.7)', transformOrigin: 'top left' }}>
        <EdgeLayer>{t2iEdges(N).map((e, i) => <Edge key={i} {...e} color="var(--border-3)" w={1.5} />)}</EdgeLayer>
        {Object.values(N).map((n, i) => <NodeCard key={i} {...n} />)}
      </div>
    </CanvasFrame>
  );
}

Object.assign(window, { CanvasRefined, CanvasBold, CanvasFlat });
