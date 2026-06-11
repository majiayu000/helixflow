// proto-shell.jsx — top bar, run dock, outputs strip, confirm modal, history panel.

function PTopBar({ canUndo, onUndo, onHistory, historyOpen, onQueue, queueDisabled, queueHint, onAgentRun, running, onInterrupt, accent }) {
  return (
    <div className="wb-top" style={{ height: 54, flexShrink: 0 }}>
      <div className="brand"><span className="brand-mark">C</span>ComfyUI Agent</div>
      <span className="divider-v" />
      <div style={{ display: 'flex', gap: 4 }}>
        <span style={{ height: 28, padding: '0 12px', display: 'inline-flex', alignItems: 'center', borderRadius: 7, background: 'var(--surface-3)', fontSize: 12.5, fontWeight: 500 }}>产品图工作流</span>
        <span style={{ height: 28, padding: '0 12px', display: 'inline-flex', alignItems: 'center', borderRadius: 7, color: 'var(--text-3)', fontSize: 12.5, cursor: 'pointer' }}>+ 新建</span>
      </div>
      <div className="top-actions">
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '0 10px', height: 32, border: '1px solid var(--border-2)', borderRadius: 8, background: 'var(--surface-2)' }}>
          <span className="endpoint" style={{ height: 24, border: 0, background: 'transparent', padding: 0 }}><Ic n="lock" s={11} /> 127.0.0.1:8188</span>
          <span className="pill pill--ok" style={{ height: 22 }}><span className="led" /> 312 节点</span>
          <span className="pill pill--live" style={{ height: 22 }}><span className="led" /> Codex</span>
        </div>
        <button className="ibtn ibtn--icon" title="撤销上次应用" onClick={onUndo} style={{ opacity: canUndo ? 1 : .4, cursor: canUndo ? 'pointer' : 'default' }}><Ic n="undo" /></button>
        <button className={`ibtn ibtn--icon ${historyOpen ? 'p-on' : ''}`} title="版本与运行历史" onClick={onHistory}>
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"><circle cx="7" cy="7" r="4.8" /><path d="M7 4.5V7l1.8 1.2" /></svg>
        </button>
        <button className="ibtn ibtn--icon" title="导出 API workflow JSON"><Ic n="export" /></button>
        <span className="divider-v" />
        <button className="btn btn--soft btn--sm" style={{ height: 32 }} onClick={onAgentRun}><Ic n="spark" s={13} fill /> Agent 运行</button>
        {running
          ? <button className="btn btn--sm" style={{ height: 32, background: 'var(--red-soft)', color: 'var(--red)', border: '1px solid var(--red-line)' }} onClick={onInterrupt}><Ic n="stop" s={12} fill /> 中断</button>
          : <button className="btn btn--primary" style={{ height: 32, opacity: queueDisabled ? .45 : 1, cursor: queueDisabled ? 'default' : 'pointer' }} title={queueDisabled ? queueHint : '提交到 /prompt'} onClick={() => !queueDisabled && onQueue()}><Ic n="play" s={13} fill /> 运行 Queue</button>}
      </div>
    </div>
  );
}

function PRunDock({ run }) {
  if (!run) return null;
  const failed = run.status === 'failed';
  const interrupted = run.status === 'interrupted';
  return (
    <div className="run-monitor" style={{ flexShrink: 0 }}>
      <div className="run-head">
        {run.status === 'running' && <span className="pill pill--live" style={{ height: 22 }}><span className="led" /> 运行中</span>}
        {run.status === 'succeeded' && <span className="pill pill--ok" style={{ height: 22 }}><span className="led" /> 已完成</span>}
        {failed && <span className="pill" style={{ height: 22, background: 'var(--red-soft)', color: 'var(--red)' }}><span className="led" style={{ background: 'var(--red)' }} /> 失败</span>}
        {interrupted && <span className="pill pill--warn" style={{ height: 22 }}><span className="led" /> 已中断</span>}
        <span className="title">{run.label}</span>
        <span className="time">{run.note}</span>
      </div>
      <div className="progress-track"><div className="progress-fill" style={{ width: `${Math.round(run.progress * 100)}%`, background: failed ? 'var(--red)' : interrupted ? 'var(--amber)' : undefined }} /></div>
      <div className="run-steps">
        {run.steps.map(s => (
          <span key={s.key} className={`run-step ${s.state === 'done' ? 'done' : s.state === 'active' ? 'active' : ''}`}
            style={s.state === 'failed' ? { background: 'var(--red-soft)', color: 'var(--red)', borderColor: 'var(--red-line)' } : null}>
            {s.state === 'done' && <Ic n="check" s={11} />}
            {s.state === 'active' && <span className="spin p-rotating" style={{ width: 10, height: 10 }} />}
            {s.state === 'failed' && <Ic n="alert" s={11} />}
            {s.label}
          </span>
        ))}
      </div>
    </div>
  );
}

function POutputs({ outputs, onSelect }) {
  if (!outputs.length) return null;
  return (
    <div className="outputs" style={{ flexShrink: 0, alignItems: 'center' }}>
      {outputs.map(o => (
        <div key={o.id} style={{ textAlign: 'center' }}>
          <div className={`output-thumb ${o.sel ? 'sel' : ''}`} onClick={() => onSelect(o.id)}
            style={{ cursor: 'pointer', background: `linear-gradient(135deg, hsl(${o.hue},42%,86%), hsl(${o.hue + 38},46%,68%))` }} />
          <div style={{ fontSize: 9.5, color: 'var(--text-3)', fontFamily: 'var(--font-mono)', marginTop: 3 }}>{o.seed}</div>
        </div>
      ))}
      <div style={{ marginLeft: 'auto', color: 'var(--text-3)', fontSize: 11.5 }}>{outputs.length} 张输出 · 点击选中</div>
    </div>
  );
}

function PConfirm({ plan, onConfirm, onCancel }) {
  if (!plan) return null;
  return (
    <div style={{ position: 'absolute', inset: 0, background: 'rgba(24,24,27,.32)', display: 'grid', placeItems: 'center', zIndex: 50 }}>
      <div className="confirm">
        <div className="confirm-head"><span className="ic"><Ic n="warn" s={14} /></span> Agent 请求运行生成</div>
        <div className="confirm-list">
          <div className="confirm-item"><span className="k">工作流版本</span><span className="v">{plan.versionLabel}</span></div>
          <div className="confirm-item"><span className="k">运行次数</span><span className="v">{plan.runs} 次（消耗本地算力）</span></div>
          <div className="confirm-item"><span className="k">待确认变更</span><span className="v" style={{ color: plan.pending ? 'var(--amber)' : 'var(--green)' }}>{plan.pending ? '有 — 将先被忽略' : '无'}</span></div>
          <div className="confirm-item"><span className="k">文件上传</span><span className="v">{plan.uploads || '无'}</span></div>
          <div className="confirm-item"><span className="k">可随时中断</span><span className="v">是（/interrupt）</span></div>
        </div>
        <div className="confirm-foot">
          <button className="btn btn--ghost btn--sm" style={{ flex: 1 }} onClick={onCancel}>取消</button>
          <button className="btn btn--primary btn--sm" style={{ flex: 1.4 }} onClick={onConfirm}><Ic n="check" s={13} /> 确认运行 {plan.runs} 次</button>
        </div>
      </div>
    </div>
  );
}

function PHistory({ open, versions, currentIdx, runs, onRestore, onClose }) {
  if (!open) return null;
  const SRC = { agent: ['Agent', 'var(--accent-soft)', 'var(--accent-ink)'], user: ['手动', 'var(--surface-3)', 'var(--text-2)'], fix: ['修复', 'var(--amber-soft)', 'var(--amber)'], restore: ['恢复', 'var(--blue-soft)', 'var(--blue)'] };
  return (
    <div style={{ position: 'absolute', top: 12, right: 12, bottom: 12, width: 272, zIndex: 40, background: 'var(--surface)', border: '1px solid var(--border-2)', borderRadius: 14, boxShadow: 'var(--sh-lg)', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div style={{ display: 'flex', alignItems: 'center', padding: '12px 14px', borderBottom: '1px solid var(--border)', fontWeight: 600, fontSize: 13 }}>
        版本与运行历史
        <button className="p-close" style={{ position: 'static', marginLeft: 'auto' }} onClick={onClose}>✕</button>
      </div>
      <div style={{ flex: 1, overflowY: 'auto', padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 14 }}>
        <div>
          <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', marginBottom: 7, letterSpacing: '.03em' }}>工作流版本</div>
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            {versions.map((v, i) => {
              const cur = i === currentIdx;
              const [lbl, bg, fg] = SRC[v.source] || SRC.user;
              return (
                <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '7px 9px', borderRadius: 8, background: cur ? 'var(--accent-soft)' : 'transparent', border: cur ? '1px solid var(--accent-line)' : '1px solid transparent' }}>
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10.5, color: cur ? 'var(--accent-ink)' : 'var(--text-3)', width: 20 }}>v{i}</span>
                  <span style={{ fontSize: 12, fontWeight: cur ? 600 : 400, flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{v.label}</span>
                  <span style={{ fontSize: 9.5, fontWeight: 600, padding: '2px 6px', borderRadius: 999, background: bg, color: fg }}>{lbl}</span>
                  {!cur && <button className="btn btn--ghost btn--sm" style={{ height: 22, padding: '0 8px', fontSize: 11 }} onClick={() => onRestore(i)}>恢复</button>}
                  {cur && <span style={{ fontSize: 10.5, color: 'var(--accent-ink)', fontWeight: 600 }}>当前</span>}
                </div>
              );
            })}
          </div>
        </div>
        {runs.length > 0 && (
          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-3)', marginBottom: 7, letterSpacing: '.03em' }}>运行记录</div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
              {runs.map((r, i) => (
                <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '6px 9px', borderRadius: 8, border: '1px solid var(--border)', fontSize: 11.5 }}>
                  <span style={{ width: 7, height: 7, borderRadius: '50%', background: r.status === 'succeeded' ? 'var(--green)' : r.status === 'failed' ? 'var(--red)' : 'var(--amber)', flexShrink: 0 }} />
                  <span style={{ flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{r.label}</span>
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-3)' }}>{r.version}</span>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

Object.assign(window, { PTopBar, PRunDock, POutputs, PConfirm, PHistory });
