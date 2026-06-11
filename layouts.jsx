// layouts.jsx — three full-workbench layout directions.

// ---------- shared chat column ----------
function ChatColumn({ presetsTop = false, showProposal = true }) {
  const presets = (
    <div className="preset-row" style={presetsTop ? { padding: '12px 16px' } : null}>
      {[['bolt', '文生图'], ['layers', '深度图 ControlNet'], ['image', '放大'], ['dice', 'Seed 试验']].map(([ic, t]) => (
        <span className="chip" key={t}><Ic n={ic} s={13} /> {t}</span>
      ))}
    </div>
  );
  return (
    <>
      <div className="chat-head">
        <Ic n="spark" s={14} c="var(--accent)" fill /> 对话
        <span className="sub">Codex CLI · 在线</span>
      </div>
      {presetsTop && presets}
      <div className="chat-msgs">
        <div className="msg msg--user">
          <div className="msg-avatar msg-avatar--user">你</div>
          <div className="msg-body">
            <div className="msg-name">你</div>
            <div className="msg-text"><span className="bubble">创建一个 SDXL 文生图工作流，1024×1024，带负面 prompt。</span></div>
          </div>
        </div>
        <div className="msg">
          <div className="msg-avatar msg-avatar--agent"><Ic n="spark" s={13} fill /></div>
          <div className="msg-body">
            <div className="msg-name">Agent</div>
            <div className="status-done" style={{ display: 'flex', alignItems: 'center', gap: 7, fontSize: 12.5, marginBottom: 10 }}>
              <Ic n="check" s={13} c="var(--green)" /> 已读取空白画布与 312 个节点定义
            </div>
            {showProposal && <PropChatCard />}
          </div>
        </div>
      </div>
      {!presetsTop && presets}
      <div className="composer">
        <div className="composer-box">
          <div className="composer-input ph">追问、修改工作流，或粘贴参考图…</div>
          <div className="composer-foot">
            <div className="left">
              <button className="ibtn ibtn--icon" style={{ height: 26, width: 26 }}><Ic n="paperclip" s={14} /></button>
            </div>
            <div className="right">
              <span className="kbd">⏎ 发送 · ⇧⏎ 换行</span>
              <button className="btn btn--primary btn--sm"><Ic n="arrowUp" s={13} /></button>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

// ---------- shared canvas (full T2I graph) ----------
function CanvasArea({ scale = 0.66, offset = { x: 28, y: 40 }, children, mode = 'View' }) {
  return (
    <div className="wb-canvas">
      <div className="canvas-grid" />
      <div className="canvas-toolbar">
        <div className="mode-seg">
          {['查看', '编辑', '审阅'].map((m, i) => <button key={m} className={(mode === '查看' && i === 0) || mode === m ? 'on' : ''}>{m}</button>)}
        </div>
        <span className="canvas-pill"><Ic n="layers" s={13} c="var(--text-3)" /> 7 节点 · 9 连线</span>
      </div>
      <T2IGraph scale={scale} offset={offset} />
      {children}
      <div className="zoom-ctl">
        <button>−</button><span>66%</span><button>＋</button>
      </div>
    </div>
  );
}

// ---------- A · 经典双栏 ----------
function LayoutClassic() {
  return (
    <div className="wb">
      <div className="wb-top">
        <div className="brand"><span className="brand-mark">C</span>ComfyUI Agent</div>
        <span className="divider-v" />
        <span className="wf-name"><span className="dot" /> 产品图工作流 <Ic n="caret" s={11} c="var(--text-3)" /></span>
        <div className="top-actions">
          <span className="endpoint"><Ic n="lock" s={12} className="lock" /> 127.0.0.1:8188</span>
          <span className="pill pill--ok"><span className="led" /> 已连接 · 312 节点</span>
          <span className="pill pill--live"><span className="led" /> Codex CLI</span>
          <span className="divider-v" />
          <button className="ibtn ibtn--icon"><Ic n="export" /></button>
          <button className="ibtn ibtn--icon ibtn--danger"><Ic n="stop" /></button>
          <button className="btn btn--primary"><Ic n="play" s={13} fill /> 运行 Queue</button>
        </div>
      </div>
      <div className="wb-body">
        <div className="wb-chat" style={{ flex: '0 0 40%' }}><ChatColumn /></div>
        <CanvasArea />
      </div>
    </div>
  );
}

// ---------- B · 画布优先 + 浮动检视 ----------
function LayoutCanvasFwd() {
  return (
    <div className="wb">
      <div className="wb-top">
        <div className="brand"><span className="brand-mark">C</span>ComfyUI Agent</div>
        <span className="divider-v" />
        <span className="wf-name"><span className="dot" /> 产品图工作流</span>
        <div className="top-actions">
          <div style={{ display: 'flex', alignItems: 'center', gap: 0, border: '1px solid var(--border-2)', borderRadius: 7, overflow: 'hidden', height: 30 }}>
            <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, padding: '0 10px', fontSize: 11.5, color: 'var(--text-2)' }}><span className="led" style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--green)' }} /> ComfyUI</span>
            <span style={{ width: 1, alignSelf: 'stretch', background: 'var(--border)' }} />
            <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, padding: '0 10px', fontSize: 11.5, color: 'var(--text-2)' }}><span className="led" style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--accent)' }} /> Codex</span>
          </div>
          <button className="ibtn ibtn--icon"><Ic n="export" /></button>
          <button className="btn btn--primary"><Ic n="play" s={13} fill /> Queue</button>
        </div>
      </div>
      <div className="wb-body">
        <div className="wb-chat" style={{ flex: '0 0 33%' }}><ChatColumn /></div>
        <CanvasArea scale={0.7} offset={{ x: 28, y: 44 }}>
          <div className="inspector" style={{ width: 232 }}>
            <div className="inspector-head">
              <div className="kicker">选中节点 · #5</div>
              <div className="title"><span className="swatch" style={{ width: 9, height: 9, borderRadius: 2, background: 'var(--t-model)' }} /> KSampler</div>
            </div>
            <div className="inspector-body">
              <div className="field"><span className="field-label">seed</span><span className="field-input">843629104821</span></div>
              <div style={{ display: 'flex', gap: 8 }}>
                <div className="field" style={{ flex: 1 }}><span className="field-label">steps</span><span className="field-input">28</span></div>
                <div className="field" style={{ flex: 1 }}><span className="field-label">cfg</span><span className="field-input">6.5</span></div>
              </div>
              <div className="field"><span className="field-label">sampler</span><span className="field-input">euler</span></div>
              <div className="field"><span className="field-label">scheduler</span><span className="field-input">normal</span></div>
            </div>
          </div>
        </CanvasArea>
      </div>
    </div>
  );
}

// ---------- C · 指挥中心（顶栏分区 + 底部运行坞）----------
function LayoutCommand() {
  return (
    <div className="wb">
      <div className="wb-top" style={{ height: 54 }}>
        <div className="brand"><span className="brand-mark">C</span>ComfyUI Agent</div>
        <span className="divider-v" />
        <div style={{ display: 'flex', gap: 4 }}>
          <span style={{ height: 28, padding: '0 12px', display: 'inline-flex', alignItems: 'center', borderRadius: 7, background: 'var(--surface-3)', fontSize: 12.5, fontWeight: 500 }}>产品图工作流</span>
          <span style={{ height: 28, padding: '0 12px', display: 'inline-flex', alignItems: 'center', borderRadius: 7, color: 'var(--text-3)', fontSize: 12.5 }}>+ 新建</span>
        </div>
        <div className="top-actions">
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '0 10px', height: 32, border: '1px solid var(--border-2)', borderRadius: 8, background: 'var(--surface-2)' }}>
            <span className="endpoint" style={{ height: 24, border: 0, background: 'transparent', padding: 0 }}><Ic n="lock" s={11} /> 127.0.0.1:8188</span>
            <span className="pill pill--ok" style={{ height: 22 }}><span className="led" /> 312</span>
            <span className="pill pill--live" style={{ height: 22 }}><span className="led" /> Codex</span>
          </div>
          <button className="ibtn ibtn--icon"><Ic n="export" /></button>
          <button className="btn btn--soft btn--sm" style={{ height: 32 }}><Ic n="spark" s={13} fill /> Agent 运行</button>
          <button className="btn btn--primary" style={{ height: 32 }}><Ic n="play" s={13} fill /> Queue</button>
        </div>
      </div>
      <div className="wb-body">
        <div className="wb-chat" style={{ flex: '0 0 38%' }}><ChatColumn presetsTop /></div>
        <div className="wb-canvas" style={{ display: 'flex', flexDirection: 'column' }}>
          <div style={{ position: 'relative', flex: 1, overflow: 'hidden' }}>
            <div className="canvas-grid" />
            <div className="canvas-toolbar">
              <div className="mode-seg"><button className="on">查看</button><button>编辑</button><button>审阅</button></div>
            </div>
            <T2IGraph scale={0.58} offset={{ x: 24, y: 34 }} />
          </div>
          {/* run monitor dock */}
          <div className="run-monitor" style={{ flexShrink: 0 }}>
            <div className="run-head">
              <span className="pill pill--live" style={{ height: 22 }}><span className="led" /> 运行中</span>
              <span className="title">KSampler · 步 18/28</span>
              <span className="time">00:07 · ~5s 剩余</span>
            </div>
            <div className="progress-track"><div className="progress-fill" style={{ width: '64%' }} /></div>
            <div className="run-steps">
              <span className="run-step done"><Ic n="check" s={11} /> Checkpoint</span>
              <span className="run-step done"><Ic n="check" s={11} /> CLIP ×2</span>
              <span className="run-step active"><span className="spin" style={{ width: 10, height: 10 }} /> KSampler</span>
              <span className="run-step">VAE Decode</span>
              <span className="run-step">Save</span>
            </div>
          </div>
          {/* outputs filmstrip */}
          <div className="outputs" style={{ flexShrink: 0 }}>
            <div className="output-thumb sel" />
            <div className="output-thumb" />
            <div className="output-thumb" />
            <div style={{ marginLeft: 'auto', alignSelf: 'center', color: 'var(--text-3)', fontSize: 11.5 }}>Run #3 · 3 张输出</div>
          </div>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, { ChatColumn, CanvasArea, LayoutClassic, LayoutCanvasFwd, LayoutCommand });
