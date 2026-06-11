// proto-canvas.jsx — interactive node canvas: pan/zoom, selection, review diff, run states.

const PC_HEAD = 31, PC_PAD = 8, PC_ROW = 26;
const pcPortY = (n, r) => n.y + PC_HEAD + PC_PAD + r * PC_ROW + PC_ROW / 2;

function PNode({ k, n, state, flag, selected, active, done, failed, onSelect }) {
  const cls = ['node', state && `node--${state}`, selected && 'p-sel', active && 'p-active', failed && 'node--err'].filter(Boolean).join(' ');
  return (
    <div className={cls} style={{ left: n.x, top: n.y, width: n.w || 188, '--swatch': n.swatch, cursor: 'pointer' }}
      onClick={(e) => { e.stopPropagation(); onSelect(k); }}>
      {flag && <span className={`node-flag ${flag}`}>{flag === 'add' ? '＋ 新增' : flag === 'upd' ? '✎ 修改' : '⚠ 失败'}</span>}
      {done && <span className="p-done"><Ic n="check" s={10} sw={2.2} /></span>}
      {active && <span className="p-spin" />}
      <div className="node-title">
        <span className="swatch" style={{ background: n.swatch }} />
        {n.title}
        <span className="p-nid">{PD_ID[k]}</span>
      </div>
      <div className="node-body">
        {n.inputs.map((p, i) => (
          <div className="io-row" key={'i' + i}>
            <span className="io-in"><Port type={p.type} />{p.label}</span>
            {n.outputs[i] && <span className="io-out"><Port type={n.outputs[i].type} />{n.outputs[i].label}</span>}
          </div>
        ))}
        {n.outputs.slice(n.inputs.length).map((p, i) => (
          <div className="io-row" key={'o' + i}>
            <span style={{ flex: 1 }} />
            <span className="io-out"><Port type={p.type} />{p.label}</span>
          </div>
        ))}
        {n.thumb && <div className="preview-thumb" />}
        {n.params.map((p, i) => (
          <div className="param-row" key={'p' + i}>
            <span className="param-k">{p.k}</span>
            <span className="param-v" style={state === 'upd' && p.k === 'model' ? { color: 'var(--amber)', fontWeight: 600 } : null}>{p.v}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function ProtoCanvas({ graph, pending, run, errorKey, selected, onSelect, styleCls = 'cv-bold' }) {
  const [view, setView] = React.useState({ x: 20, y: 16, z: 0.64 });
  const drag = React.useRef(null);

  // what to draw: pending proposal preview takes precedence
  const draw = pending ? pending.graph : graph;
  const nodes = draw.nodes, edges = draw.edges;
  const appliedEdgeSet = new Set(graph.edges.map(e => e.f.join(',') + '>' + e.t.join(',')));
  const nodeCount = Object.keys(nodes).length;

  const onDown = (e) => { drag.current = { sx: e.clientX, sy: e.clientY, ox: view.x, oy: view.y }; };
  const onMove = (e) => {
    if (!drag.current) return;
    setView(v => ({ ...v, x: drag.current.ox + e.clientX - drag.current.sx, y: drag.current.oy + e.clientY - drag.current.sy }));
  };
  const onUp = () => { drag.current = null; };

  return (
    <div className={`p-canvas ${styleCls}`} onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onPointerLeave={onUp}
      onClick={() => onSelect(null)}>
      <div className="canvas-grid" />
      {/* toolbar */}
      <div className="canvas-toolbar" onPointerDown={e => e.stopPropagation()}>
        <div className="mode-seg">
          <button className={!pending ? 'on' : ''}>查看</button>
          <button>编辑</button>
          <button className={pending ? 'on' : ''}>审阅</button>
        </div>
        {nodeCount > 0 && <span className="canvas-pill"><Ic n="layers" s={13} c="var(--text-3)" /> {nodeCount} 节点 · {edges.length} 连线</span>}
        {pending && <span className="pill pill--warn" style={{ boxShadow: 'var(--sh-sm)' }}><span className="led" /> 待确认的图变更 — 预览中</span>}
      </div>

      {/* world */}
      <div style={{ position: 'absolute', left: view.x, top: view.y, transform: `scale(${view.z})`, transformOrigin: 'top left' }}>
        {/* conditioning group (bold style) */}
        {styleCls === 'cv-bold' && nodes.pos && (
          <div className="cv-group" style={{ left: 250, top: 22, width: 224, height: 300 }}>
            <span className="cv-group-label">Conditioning</span>
          </div>
        )}
        {styleCls === 'cv-bold' && nodes.img && (
          <div className="cv-group" style={{ left: 12, top: 412, width: 462, height: 310, background: 'color-mix(in srgb, var(--t-image) 4%, transparent)' }}>
            <span className="cv-group-label">ControlNet Depth</span>
          </div>
        )}
        <EdgeLayer>
          {edges.map((e, i) => {
            const fn = nodes[e.f[0]], tn = nodes[e.t[0]];
            if (!fn || !tn) return null;
            const isNew = pending && !appliedEdgeSet.has(e.f.join(',') + '>' + e.t.join(','));
            return <Edge key={i}
              x1={fn.x + (fn.w || 188)} y1={pcPortY(fn, e.f[1])}
              x2={tn.x} y2={pcPortY(tn, e.t[1])}
              color={TYPE_COLOR[e.type] || 'var(--border-3)'} w={3} dashed={isNew} />;
          })}
        </EdgeLayer>
        {Object.entries(nodes).map(([k, n]) => {
          const isAdd = pending && pending.addedKeys.includes(k);
          const isUpd = pending && pending.updatedKeys.includes(k);
          return <PNode key={k} k={k} n={n}
            state={isAdd ? 'add' : isUpd ? 'upd' : null}
            flag={isAdd ? 'add' : isUpd ? 'upd' : (errorKey === k ? 'err' : null)}
            failed={errorKey === k}
            selected={selected === k}
            active={run && run.activeKey === k}
            done={run && run.doneKeys.includes(k)}
            onSelect={onSelect} />;
        })}
      </div>

      {/* empty state */}
      {nodeCount === 0 && (
        <div className="empty-canvas">
          <div className="empty-card">
            <div className="empty-icon"><Ic n="layers" s={22} /></div>
            <div className="empty-title">空白工作流</div>
            <div className="empty-sub">在左侧用一句话描述你想要的图像，<br />Agent 会生成可审核的节点图。</div>
          </div>
        </div>
      )}

      {/* zoom */}
      <div className="zoom-ctl" onPointerDown={e => e.stopPropagation()}>
        <button onClick={(e) => { e.stopPropagation(); setView(v => ({ ...v, z: Math.max(0.4, +(v.z - 0.1).toFixed(2)) })); }}>−</button>
        <span>{Math.round(view.z * 100)}%</span>
        <button onClick={(e) => { e.stopPropagation(); setView(v => ({ ...v, z: Math.min(1.4, +(v.z + 0.1).toFixed(2)) })); }}>＋</button>
      </div>

      {/* inspector */}
      {selected && nodes[selected] && (
        <div className="inspector p-inspector" onPointerDown={e => e.stopPropagation()} onClick={e => e.stopPropagation()}>
          <div className="inspector-head" style={{ position: 'relative' }}>
            <div className="kicker">选中节点 · {PD_ID[selected]}</div>
            <div className="title"><span style={{ width: 9, height: 9, borderRadius: 2, background: nodes[selected].swatch }} /> {nodes[selected].title}</div>
            <button className="p-close" onClick={() => onSelect(null)}>✕</button>
          </div>
          <div className="inspector-body">
            {nodes[selected].params.length === 0 && <div style={{ color: 'var(--text-3)', fontSize: 12 }}>该节点没有可编辑参数。</div>}
            {nodes[selected].params.map((p, i) => (
              <div className="field" key={i}>
                <span className="field-label">{p.k}</span>
                <span className="field-input">{p.v}</span>
              </div>
            ))}
            {errorKey === selected && (
              <div style={{ background: 'var(--red-soft)', border: '1px solid var(--red-line)', borderRadius: 7, padding: '8px 10px', fontSize: 11.5, color: 'var(--red)', lineHeight: 1.5 }}>
                上次运行在此节点失败：模型文件缺失。
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

Object.assign(window, { ProtoCanvas, PNode, pcPortY });
