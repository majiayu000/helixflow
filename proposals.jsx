// proposals.jsx — three presentations of an Agent graph proposal.

// shared change list data
const CHANGES_T2I = [
  { kind: 'add', lbl: 'Load Checkpoint', meta: '#1' },
  { kind: 'add', lbl: 'CLIP Text Encode ×2', meta: '#2,3' },
  { kind: 'add', lbl: 'Empty Latent Image', meta: '#4' },
  { kind: 'add', lbl: 'KSampler', meta: '#5' },
  { kind: 'add', lbl: 'VAE Decode → Save Image', meta: '#6,7' },
];

// ============ A · inline structured chat card (default) ============
function PropChatCard({ title = 'SDXL 文生图工作流', summary = '新建 7 个节点，连成标准文生图链路：Checkpoint → 双 CLIP 编码 → KSampler → VAE Decode → Save。', changes = CHANGES_T2I, compact = false }) {
  return (
    <div className="prop">
      <div className="prop-head">
        <span className="prop-badge"><Ic n="spark" s={11} fill /> 图变更提议</span>
        <span className="prop-title">{title}</span>
      </div>
      <div className="prop-summary">{summary}</div>
      <div className="prop-changes">
        {changes.map((c, i) => (
          <div className="change-row" key={i}>
            <span className={`change-tag ${c.kind}`}>{c.kind === 'add' ? '+' : c.kind === 'upd' ? '~' : '−'}</span>
            <span className="lbl">{c.lbl}</span>
            <span className="meta">{c.meta}</span>
          </div>
        ))}
      </div>
      <div className="prop-actions">
        <button className="btn btn--primary btn--sm"><Ic n="check" s={13} /> 应用到画布</button>
        <button className="btn btn--ghost btn--sm">查看 Diff</button>
        <span className="spacer" />
        <button className="btn btn--quiet btn--sm">忽略</button>
      </div>
    </div>
  );
}

// ============ B · canvas-first lightweight bar ============
function PropCanvasBar() {
  // mini canvas with new nodes highlighted green + floating action toolbar
  const N = t2iNodes();
  ['ckpt', 'pos', 'neg', 'latent', 'ks', 'vae', 'save'].forEach(k => N[k].state = 'add');
  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* one-line chat note */}
      <div style={{ display: 'flex', gap: 10, padding: '14px 16px', borderBottom: '1px solid var(--border)', background: 'var(--surface)' }}>
        <div className="msg-avatar msg-avatar--agent"><Ic n="spark" s={13} fill /></div>
        <div className="msg-body">
          <div className="msg-name">Agent</div>
          <div className="msg-text dim">已在画布上预览 <b style={{ color: 'var(--green)' }}>7 个新节点</b> 与 9 条连线。在右侧确认即可应用。</div>
        </div>
      </div>
      {/* canvas with green diff + floating toolbar */}
      <div style={{ position: 'relative', flex: 1, overflow: 'hidden' }}>
        <div className="canvas-grid" />
        <T2IGraph scale={0.62} offset={{ x: 18, y: 14 }} nodes={N} edgeWidth={2.4} />
        <div style={{ position: 'absolute', left: '50%', bottom: 18, transform: 'translateX(-50%)', display: 'flex', alignItems: 'center', gap: 10, background: 'var(--surface)', border: '1px solid var(--border-2)', borderRadius: 999, boxShadow: 'var(--sh-pop)', padding: '7px 8px 7px 16px' }}>
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 7, fontSize: 12.5, fontWeight: 500 }}>
            <span style={{ width: 8, height: 8, borderRadius: 2, background: 'var(--green)' }} />
            7 新增 · 9 连线
          </span>
          <span className="divider-v" />
          <button className="btn btn--quiet btn--sm">忽略</button>
          <button className="btn btn--primary btn--sm"><Ic n="check" s={13} /> 应用变更</button>
        </div>
      </div>
    </div>
  );
}

// ============ C · power-user grouped diff panel ============
function PropDiffPanel() {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', background: 'var(--surface)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '13px 16px', borderBottom: '1px solid var(--border)' }}>
        <span className="prop-badge"><Ic n="layers" s={11} /> Inspect</span>
        <span style={{ fontWeight: 600 }}>SDXL 文生图工作流</span>
        <div style={{ marginLeft: 'auto', display: 'flex', background: 'var(--surface-2)', border: '1px solid var(--border-2)', borderRadius: 7, padding: 2 }}>
          <button className="mode-seg-btn on" style={segOn}>摘要</button>
          <button style={segOff}>JSON Diff</button>
        </div>
      </div>
      <div style={{ flex: 1, overflow: 'hidden', padding: '14px 16px', display: 'flex', flexDirection: 'column', gap: 16 }}>
        <DiffGroup title="新增节点" count={7} kind="add" rows={[
          ['Load Checkpoint', 'sdxl_base.safetensors'],
          ['CLIP Text Encode (Positive)', 'a product shot, studio…'],
          ['CLIP Text Encode (Negative)', 'blurry, low quality…'],
          ['KSampler', 'euler · 28 steps · cfg 6.5'],
        ]} />
        <DiffGroup title="新增连线" count={9} kind="add" rows={[
          ['Checkpoint.MODEL', '→ KSampler.model'],
          ['Checkpoint.CLIP', '→ CLIP Encode ×2'],
          ['KSampler.LATENT', '→ VAE Decode.samples'],
        ]} mono />
        <DiffGroup title="运行计划" count={1} kind="upd" rows={[
          ['queue', '提交 1 次到 /prompt'],
        ]} />
      </div>
      <div style={{ display: 'flex', gap: 8, padding: '12px 16px', borderTop: '1px solid var(--border)', background: 'var(--surface-2)' }}>
        <button className="btn btn--primary btn--sm"><Ic n="check" s={13} /> 应用到画布</button>
        <button className="btn btn--ghost btn--sm">仅应用部分…</button>
        <span style={{ flex: 1 }} />
        <button className="btn btn--quiet btn--sm">忽略</button>
      </div>
    </div>
  );
}
const segOn = { height: 24, padding: '0 11px', border: 0, background: 'var(--accent-soft)', color: 'var(--accent-ink)', borderRadius: 5, fontSize: 11.5, fontWeight: 500, fontFamily: 'var(--font-sans)', cursor: 'pointer' };
const segOff = { height: 24, padding: '0 11px', border: 0, background: 'transparent', color: 'var(--text-2)', borderRadius: 5, fontSize: 11.5, fontWeight: 500, fontFamily: 'var(--font-sans)', cursor: 'pointer' };

function DiffGroup({ title, count, kind, rows, mono }) {
  const col = kind === 'add' ? 'var(--green)' : kind === 'upd' ? 'var(--amber)' : 'var(--red)';
  const soft = kind === 'add' ? 'var(--green-soft)' : kind === 'upd' ? 'var(--amber-soft)' : 'var(--red-soft)';
  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
        <span style={{ fontSize: 11.5, fontWeight: 600, color: 'var(--text-2)' }}>{title}</span>
        <span style={{ height: 17, minWidth: 17, padding: '0 5px', borderRadius: 999, background: soft, color: col, fontSize: 10.5, fontWeight: 700, display: 'grid', placeItems: 'center' }}>{count}</span>
      </div>
      <div style={{ border: '1px solid var(--border)', borderRadius: 8, overflow: 'hidden' }}>
        {rows.map((r, i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 9, padding: '7px 11px', borderTop: i ? '1px solid var(--border)' : 0, background: i % 2 ? 'var(--surface)' : 'var(--surface-2)' }}>
            <span style={{ width: 4, alignSelf: 'stretch', borderRadius: 2, background: col, flexShrink: 0 }} />
            <span style={{ fontSize: 12, fontWeight: 500 }}>{r[0]}</span>
            <span style={{ marginLeft: 'auto', fontSize: 11, color: 'var(--text-3)', fontFamily: mono ? 'var(--font-mono)' : 'var(--font-sans)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: 180 }}>{r[1]}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

Object.assign(window, { PropChatCard, PropCanvasBar, PropDiffPanel, CHANGES_T2I });
