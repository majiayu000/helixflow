// bits.jsx — shared mock primitives for the ComfyUI Agent Workbench exploration
// Exported to window at the end.

const TYPE_COLOR = {
  MODEL: 'var(--t-model)', CLIP: 'var(--t-clip)', VAE: 'var(--t-vae)',
  LATENT: 'var(--t-latent)', CONDITIONING: 'var(--t-cond)', IMAGE: 'var(--t-image)',
  MASK: 'var(--t-mask)', INT: 'var(--t-num)', FLOAT: 'var(--t-num)',
  CONTROL_NET: 'var(--t-model)', UPSCALE_MODEL: 'var(--t-model)',
};

// ---- icon set ----
const I = {
  send: <path d="M3.4 11.5 12.5 7 3.4 2.5l1.5 4L9 7l-4.1.5z" />,
  arrowUp: <path d="M7 11V3M3.5 6.5 7 3l3.5 3.5" />,
  spark: <path d="M7 1.5l1.2 3.3 3.3 1.2-3.3 1.2L7 12.5 5.8 7.2 2.5 6l3.3-1.2L7 1.5z" />,
  link: <path d="M5.5 8.5 8.5 5.5M6 4l.8-.8a2.1 2.1 0 0 1 3 3l-.8.8M8 10l-.8.8a2.1 2.1 0 0 1-3-3L5 7" />,
  lock: <g><rect x="3.2" y="6.2" width="7.6" height="5.3" rx="1.2"/><path d="M4.8 6.2V4.6a2.2 2.2 0 0 1 4.4 0v1.6"/></g>,
  check: <path d="M3 7.2 5.6 9.8 11 4.2" />,
  plus: <path d="M7 3v8M3 7h8" />,
  image: <g><rect x="2.5" y="3" width="9" height="8" rx="1.3"/><circle cx="5.2" cy="5.7" r="1"/><path d="M2.8 9.5 5.5 7l2 1.6L9 7l2.2 2.2"/></g>,
  stop: <rect x="3.5" y="3.5" width="7" height="7" rx="1.3" />,
  play: <path d="M4.5 3v8l6-4z" />,
  layers: <g><path d="M7 2.5 12 5 7 7.5 2 5z"/><path d="M2 7.5 7 10l5-2.5"/></g>,
  export: <g><path d="M7 9V2.5M4.5 5 7 2.5 9.5 5"/><path d="M3 9.5v1.5a.8.8 0 0 0 .8.8h6.4a.8.8 0 0 0 .8-.8V9.5"/></g>,
  undo: <path d="M4.5 6.5 2.5 8.5 4.5 10.5M2.8 8.5h5.4a2.7 2.7 0 0 0 0-5.4H6" />,
  search: <g><circle cx="6.2" cy="6.2" r="3"/><path d="M8.4 8.4 11 11"/></g>,
  chevron: <path d="M5 4l3 3-3 3" />,
  caret: <path d="M4 5.5 7 8.5l3-3" />,
  warn: <g><path d="M7 2.8 12 11H2z"/><path d="M7 6v2.2M7 9.6v.1"/></g>,
  alert: <g><circle cx="7" cy="7" r="4.5"/><path d="M7 4.8V7.4M7 9.2v.1"/></g>,
  bolt: <path d="M7.5 2 3.5 8h3l-1 4 4-6h-3l1-4z" />,
  dice: <g><rect x="2.5" y="2.5" width="9" height="9" rx="2"/><circle cx="5" cy="5" r=".9"/><circle cx="9" cy="9" r=".9"/><circle cx="7" cy="7" r=".9"/></g>,
  grid: <g><rect x="2.5" y="2.5" width="3.5" height="3.5" rx="1"/><rect x="8" y="2.5" width="3.5" height="3.5" rx="1"/><rect x="2.5" y="8" width="3.5" height="3.5" rx="1"/><rect x="8" y="8" width="3.5" height="3.5" rx="1"/></g>,
  paperclip: <path d="M9.5 5.5 5.8 9.2a1.6 1.6 0 0 1-2.3-2.3l3.8-3.8a2.6 2.6 0 0 1 3.7 3.7L7.2 10.4" />,
  refresh: <path d="M10.5 4.5A4 4 0 1 0 11 8M10.5 2.5v2h-2" />,
};
function Ic({ n, s = 14, c, sw = 1.5, fill = false, style }) {
  return (
    <svg width={s} height={s} viewBox="0 0 14 14" fill={fill ? 'currentColor' : 'none'}
      stroke={fill ? 'none' : 'currentColor'} strokeWidth={sw} strokeLinecap="round" strokeLinejoin="round"
      style={{ color: c, flexShrink: 0, ...style }}>{I[n]}</svg>
  );
}

// ---- ports ----
function Port({ type = 'IMAGE' }) {
  return <span className="port" style={{ color: TYPE_COLOR[type] || 'var(--text-3)' }} />;
}

// ---- node card ----
function NodeCard({ title, swatch = 'var(--accent)', inputs = [], outputs = [], params = [],
  thumb = false, state = null, flag = null, x = 0, y = 0, w = 188, dim = false }) {
  const cls = ['node', state && `node--${state}`].filter(Boolean).join(' ');
  return (
    <div className={cls} style={{ left: x, top: y, width: w, opacity: dim ? .5 : 1, '--swatch': swatch }}>
      {flag && <span className={`node-flag ${flag.kind}`}>{flag.kind === 'add' ? '＋ ' : flag.kind === 'upd' ? '✎ ' : '⚠ '}{flag.text}</span>}
      <div className="node-title">
        <span className="swatch" style={{ background: swatch }} />
        {title}
      </div>
      <div className="node-body">
        {inputs.map((p, i) => (
          <div className="io-row" key={'i' + i}>
            <span className="io-in"><Port type={p.type} />{p.label}</span>
            {outputs[i] && <span className="io-out"><Port type={outputs[i].type} />{outputs[i].label}</span>}
          </div>
        ))}
        {outputs.slice(inputs.length).map((p, i) => (
          <div className="io-row" key={'o' + i}>
            <span style={{ flex: 1 }} />
            <span className="io-out"><Port type={p.type} />{p.label}</span>
          </div>
        ))}
        {thumb && <div className="preview-thumb" />}
        {params.map((p, i) => (
          <div className="param-row" key={'p' + i}>
            <span className="param-k">{p.k}</span>
            <span className="param-v" style={p.hl ? { color: 'var(--amber)' } : null}>{p.v}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ---- bezier edge (horizontal) ----
function Edge({ x1, y1, x2, y2, color = 'var(--border-3)', dashed = false, w = 2 }) {
  const dx = Math.max(40, Math.abs(x2 - x1) * 0.5);
  const d = `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
  return <path d={d} fill="none" stroke={color} strokeWidth={w} strokeDasharray={dashed ? '5 5' : null} strokeLinecap="round" />;
}
function EdgeLayer({ children, style }) {
  return (
    <svg style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', overflow: 'visible', pointerEvents: 'none', ...style }}>
      {children}
    </svg>
  );
}

Object.assign(window, { Ic, Port, NodeCard, Edge, EdgeLayer, TYPE_COLOR });
