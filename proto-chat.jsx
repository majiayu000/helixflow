// proto-chat.jsx — chat pane: messages, statuses, proposal cards, run-plan confirm, composer.

function PropCard({ proposal, state, onApply, onDismiss }) {
  const [open, setOpen] = React.useState(false);
  return (
    <div className="prop" style={state === 'dismissed' ? { opacity: .55 } : null}>
      <div className="prop-head">
        <span className="prop-badge"><Ic n="spark" s={11} fill /> 图变更提议</span>
        <span className="prop-title">{proposal.title}</span>
      </div>
      <div className="prop-summary">{proposal.summary}</div>
      <div className="prop-changes">
        {proposal.changes.map((c, i) => (
          <div className="change-row" key={i}>
            <span className={`change-tag ${c.kind}`}>{c.kind === 'add' ? '+' : c.kind === 'upd' ? '~' : '−'}</span>
            <span className="lbl">{c.lbl}</span>
            <span className="meta">{c.meta}</span>
          </div>
        ))}
        {open && (
          <div style={{ marginTop: 6, padding: '8px 10px', background: 'var(--surface-2)', border: '1px solid var(--border)', borderRadius: 7, fontFamily: 'var(--font-mono)', fontSize: 10.5, color: 'var(--text-2)', lineHeight: 1.7 }}>
            {proposal.addedKeys.map(k => <div key={k}>+ addNode {PD_ID[k]} {PD_NODES[k].title}</div>)}
            {proposal.updatedKeys.map(k => <div key={k} style={{ color: 'var(--amber)' }}>~ update {PD_ID[k]} {PD_NODES[k].title}.model</div>)}
            {proposal.kind === 'modify' && <div style={{ color: 'var(--red)' }}>− removeEdge #2.COND → #5.positive</div>}
          </div>
        )}
      </div>
      <div className="prop-actions">
        {state === 'pending' && <>
          <button className="btn btn--primary btn--sm" onClick={onApply}><Ic n="check" s={13} /> 应用到画布</button>
          <button className="btn btn--ghost btn--sm" onClick={() => setOpen(o => !o)}>{open ? '收起' : '查看 Diff'}</button>
          <span className="spacer" />
          <button className="btn btn--quiet btn--sm" onClick={onDismiss}>忽略</button>
        </>}
        {state === 'applied' && <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12, color: 'var(--green)', fontWeight: 500 }}><Ic n="check" s={13} /> 已应用 · {proposal.versionLabel}</span>}
        {state === 'dismissed' && <span style={{ fontSize: 12, color: 'var(--text-3)' }}>已忽略</span>}
      </div>
    </div>
  );
}

function ErrorCard({ error }) {
  const [raw, setRaw] = React.useState(false);
  return (
    <div style={{ border: '1px solid var(--red-line)', borderRadius: 10, overflow: 'hidden', background: 'var(--surface)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '10px 12px', background: 'var(--red-soft)', color: 'var(--red)', fontWeight: 600, fontSize: 12.5 }}>
        <Ic n="alert" s={14} /> {error.title}
      </div>
      <div style={{ padding: '10px 12px', fontSize: 12.5, color: 'var(--text)', lineHeight: 1.55 }}>{error.human}</div>
      <div style={{ padding: '0 12px 10px' }}>
        <button className="btn btn--quiet btn--sm" style={{ height: 22, padding: 0, fontSize: 11.5, color: 'var(--text-3)' }} onClick={() => setRaw(r => !r)}>{raw ? '收起原始错误' : '查看原始错误'}</button>
        {raw && <div style={{ marginTop: 6, padding: '8px 10px', background: 'var(--surface-2)', border: '1px solid var(--border)', borderRadius: 7, fontFamily: 'var(--font-mono)', fontSize: 10.5, color: 'var(--red)', lineHeight: 1.6, wordBreak: 'break-all' }}>{error.raw}</div>}
      </div>
    </div>
  );
}

function SweepCard({ best }) {
  return (
    <div style={{ border: '1px solid var(--border-2)', borderRadius: 10, padding: 12, background: 'var(--surface)' }}>
      <div style={{ display: 'flex', gap: 8, marginBottom: 9 }}>
        {SWEEP_HUES.map((h, i) => (
          <div key={i} style={{ flex: 1 }}>
            <div style={{ aspectRatio: '1', borderRadius: 7, background: `linear-gradient(135deg, hsl(${h},42%,86%), hsl(${h + 38},46%,68%))`, border: i === best ? '2px solid var(--accent)' : '1px solid var(--border-2)', boxShadow: i === best ? '0 0 0 3px var(--accent-soft)' : 'none', position: 'relative' }}>
              {i === best && <span style={{ position: 'absolute', top: 4, right: 4, width: 16, height: 16, borderRadius: '50%', background: 'var(--accent)', color: '#fff', display: 'grid', placeItems: 'center' }}><Ic n="check" s={9} sw={2.5} /></span>}
            </div>
            <div style={{ fontSize: 10, color: 'var(--text-3)', fontFamily: 'var(--font-mono)', textAlign: 'center', marginTop: 4 }}>{SWEEP_SEEDS[i]}</div>
          </div>
        ))}
      </div>
      <div style={{ fontSize: 12.5, color: 'var(--text)', lineHeight: 1.55 }}>4 个 seed 已跑完。<b>第 {best + 1} 张</b>（seed {SWEEP_SEEDS[best]}）构图最稳：主体居中、景深层次清晰。已在输出条中选中，其余保留可对比。</div>
    </div>
  );
}

function ChatPane({ messages, presets, onPreset, onSend, onApply, onDismiss, onRequestRun, busy }) {
  const [draft, setDraft] = React.useState('');
  const scrollRef = React.useRef(null);
  React.useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages]);

  const send = () => {
    const t = draft.trim();
    if (!t || busy) return;
    setDraft('');
    onSend(t);
  };

  return (
    <div className="wb-chat" style={{ flex: 'none', width: '100%', height: '100%' }}>
      <div className="chat-head">
        <Ic n="spark" s={14} c="var(--accent)" fill /> 对话
        <span className="sub">Codex CLI · 在线</span>
      </div>
      <div className="preset-row" style={{ padding: '10px 16px', borderBottom: '1px solid var(--border)', flexWrap: 'nowrap', overflowX: 'auto' }}>
        {presets.map(p => (
          <button className="chip" key={p.id} onClick={() => !busy && onPreset(p.id)} style={{ border: '1px solid var(--border-2)', background: 'var(--surface)', fontFamily: 'var(--font-sans)', opacity: busy ? .5 : 1, flexShrink: 0 }}>
            <Ic n={p.icon} s={13} /> {p.label}
          </button>
        ))}
      </div>
      <div className="chat-msgs" style={{ overflowY: 'auto' }} ref={scrollRef}>
        {messages.length === 0 && (
          <div style={{ margin: 'auto', textAlign: 'center', color: 'var(--text-3)', fontSize: 12.5, lineHeight: 1.7 }}>
            还没有消息。<br />描述你想生成的图像，或点击上方快捷指令。
          </div>
        )}
        {messages.map(m => {
          if (m.role === 'sys') return (
            <div key={m.id} style={{ textAlign: 'center', fontSize: 11.5, color: 'var(--text-3)', display: 'flex', alignItems: 'center', gap: 10 }}>
              <span style={{ flex: 1, height: 1, background: 'var(--border)' }} />
              {m.text}
              <span style={{ flex: 1, height: 1, background: 'var(--border)' }} />
            </div>
          );
          const isUser = m.role === 'user';
          return (
            <div className={`msg ${isUser ? 'msg--user' : ''}`} key={m.id}>
              <div className={`msg-avatar ${isUser ? 'msg-avatar--user' : 'msg-avatar--agent'}`}>{isUser ? '你' : <Ic n="spark" s={13} fill />}</div>
              <div className="msg-body">
                <div className="msg-name">{isUser ? '你' : 'Agent'}</div>
                {m.status ? (
                  <div className="status-line"><span className="spin p-rotating" /> {m.text}</div>
                ) : (
                  <>
                    {m.text && <div className={`msg-text ${isUser ? '' : 'dim'}`} style={{ marginBottom: (m.proposal || m.error || m.sweep || m.runplan) ? 9 : 0 }}>
                      {isUser ? <span className="bubble">{m.text}</span> : m.text}
                    </div>}
                    {m.attachment && <div style={{ display: 'inline-flex', alignItems: 'center', gap: 7, marginTop: 6, padding: '5px 10px', border: '1px solid var(--border-2)', borderRadius: 7, background: 'var(--surface-2)', fontSize: 11.5, color: 'var(--text-2)' }}><Ic n="image" s={13} /> {m.attachment}</div>}
                    {m.proposal && <PropCard proposal={m.proposal} state={m.propState} onApply={() => onApply(m.id)} onDismiss={() => onDismiss(m.id)} />}
                    {m.error && <ErrorCard error={m.error} />}
                    {m.runplan && (
                      <div style={{ border: '1px solid var(--accent-line)', borderRadius: 10, padding: '11px 13px', background: 'linear-gradient(180deg, var(--accent-soft), var(--surface) 70%)' }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 7, fontWeight: 600, fontSize: 12.5, marginBottom: 5 }}><Ic n="dice" s={14} c="var(--accent)" /> 运行计划 · seed 试验 ×4</div>
                        <div style={{ fontSize: 12, color: 'var(--text-2)', lineHeight: 1.6, marginBottom: 9 }}>固定其余参数，用 4 个随机 seed 各运行一次，跑完后对比构图并给出推荐。</div>
                        {m.runplanState === 'pending'
                          ? <button className="btn btn--primary btn--sm" onClick={() => onRequestRun(m.id)}><Ic n="play" s={12} fill /> 请求运行（需你确认）</button>
                          : <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12, color: 'var(--green)', fontWeight: 500 }}><Ic n="check" s={13} /> 已确认执行</span>}
                      </div>
                    )}
                    {m.sweep && <SweepCard best={m.sweep.best} />}
                  </>
                )}
              </div>
            </div>
          );
        })}
      </div>
      <div className="composer">
        <div className="composer-box">
          <textarea
            value={draft}
            onChange={e => setDraft(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); send(); }
            }}
            placeholder="描述想要的图像，或对当前工作流提出修改…"
            rows={1}
            style={{ border: 0, outline: 'none', resize: 'none', font: 'inherit', fontFamily: 'var(--font-sans)', fontSize: 13, color: 'var(--text)', background: 'transparent', minHeight: 22, maxHeight: 96 }}
          />
          <div className="composer-foot">
            <div className="left">
              <button className="ibtn ibtn--icon" style={{ height: 26, width: 26 }}><Ic n="paperclip" s={14} /></button>
            </div>
            <div className="right">
              <span className="kbd">⏎ 发送 · ⇧⏎ 换行</span>
              <button className="btn btn--primary btn--sm" onClick={send} style={{ opacity: busy ? .5 : 1 }}><Ic n="arrowUp" s={13} /></button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, { ChatPane, PropCard, ErrorCard, SweepCard });
