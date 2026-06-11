// proto-app.jsx — the orchestrating state machine that wires chat + canvas + shell together.

const { useState, useRef, useCallback } = React;
let MID = 0; const nid = () => 'm' + (++MID);

const PRESETS = [
  { id: 'create', icon: 'bolt', label: '建文生图' },
  { id: 'controlnet', icon: 'layers', label: 'ControlNet' },
  { id: 'sweep', icon: 'dice', label: 'Seed 试验' },
  { id: 'fix', icon: 'refresh', label: '修复报错' },
];

const wait = (ms) => new Promise(r => setTimeout(r, ms));

function ProtoApp() {
  // version stack: each entry { graph, label, source }
  const [versions, setVersions] = useState([{ graph: GRAPH_EMPTY, label: '空白工作流', source: 'user' }]);
  const [curIdx, setCurIdx] = useState(0);
  const graph = versions[curIdx].graph;

  const [messages, setMessages] = useState([]);
  const [pending, setPending] = useState(null);     // proposal being previewed on canvas
  const [run, setRun] = useState(null);             // run dock state
  const [errorKey, setErrorKey] = useState(null);   // failed node on canvas
  const [selected, setSelected] = useState(null);
  const [outputs, setOutputs] = useState([]);
  const [confirm, setConfirm] = useState(null);     // confirm modal plan
  const [historyOpen, setHistoryOpen] = useState(false);
  const [runs, setRuns] = useState([]);
  const [busy, setBusy] = useState(false);
  const runRef = useRef(null);

  const pushMsg = (m) => { const id = nid(); setMessages(prev => [...prev, { id, ...m }]); return id; };
  const patchMsg = (id, patch) => setMessages(prev => prev.map(m => m.id === id ? { ...m, ...patch } : m));

  const commitVersion = (graph, label, source) => {
    setVersions(prev => {
      const next = [...prev.slice(0, curIdx + 1), { graph, label, source }];
      setCurIdx(next.length - 1);
      return next;
    });
  };

  // ---- agent "thinking" sequence then a payload message ----
  const agentThink = async (statuses, finalMsg) => {
    setBusy(true);
    const sid = pushMsg({ role: 'agent', text: statuses[0], status: true });
    for (let i = 1; i < statuses.length; i++) { await wait(620); patchMsg(sid, { text: statuses[i] }); }
    await wait(640);
    setMessages(prev => prev.filter(m => m.id !== sid));
    pushMsg(finalMsg);
    setBusy(false);
  };

  // ---- flows ----
  const flowCreate = async (userText, opts = {}) => {
    pushMsg({ role: 'user', text: userText || '创建一个 SDXL 文生图工作流，1024×1024，带负面 prompt。' });
    if (opts.silent) {
      await agentThink(['正在初始化基础文生图链路…'], { role: 'agent', text: '已为你搭好基础 SDXL 文生图工作流（7 节点）作为起点。' });
      commitVersion(PROPOSAL_1.graph, PROPOSAL_1.versionLabel, 'agent');
      setPending(null);
      return;
    }
    await agentThink(AGENT_STATUSES, { role: 'agent', text: '我规划了一个标准 SDXL 文生图链路。预览已在画布上高亮，确认后即可应用：', proposal: PROPOSAL_1, propState: 'pending' });
    setPending(PROPOSAL_1); setHistoryOpen(false);
  };

  const flowControlNet = async (userText) => {
    if (curIdx === 0) { await flowCreate('先帮我建一个基础文生图工作流。', { silent: true }); await wait(350); }
    pushMsg({ role: 'user', text: userText || '在这基础上加 ControlNet depth，用这张参考图控制构图。', attachment: 'ref_depth.png' });
    await agentThink(['正在读取当前工作流（7 节点）…', '正在分析参考图…', '正在规划增量变更…'],
      { role: 'agent', text: '会在不破坏现有节点的前提下，插入一条 depth ControlNet 分支：', proposal: PROPOSAL_2, propState: 'pending' });
    setPending(PROPOSAL_2);
  };

  const flowSweep = async (userText) => {
    if (curIdx === 0) { await flowCreate('先帮我建一个基础文生图工作流。', { silent: true }); await wait(350); }
    pushMsg({ role: 'user', text: userText || '用 4 个不同 seed 各跑一次，挑构图最好的。' });
    await agentThink(['正在读取 KSampler 参数…', '正在准备 seed 试验计划…'],
      { role: 'agent', text: '我建议跑一次 seed 试验。这会真实占用本地算力，需你确认后再执行：', runplan: true, runplanState: 'pending' });
  };

  const flowFix = async () => {
    if (curIdx === 0) { await flowCreate('先帮我建一个基础文生图工作流。', { silent: true }); await wait(350); }
    if (!graph.nodes.cnl) { await flowControlNet('加 ControlNet depth。'); }
  };

  // ---- proposal apply / dismiss ----
  const applyProposal = (msgId) => {
    const m = messages.find(x => x.id === msgId); if (!m) return;
    const p = m.proposal;
    const src = p.kind === 'fix' ? 'fix' : 'agent';
    commitVersion(p.graph, p.versionLabel, src);
    setPending(null); setErrorKey(null);
    patchMsg(msgId, { propState: 'applied' });
    pushMsg({ role: 'sys', text: `已应用 · 新版本 ${p.versionLabel}` });
  };
  const dismissProposal = (msgId) => { setPending(null); patchMsg(msgId, { propState: 'dismissed' }); };

  // ---- queue run (normal) ----
  const doRun = async (script, label, verLabel) => {
    setBusy(true); setHistoryOpen(false); setErrorKey(null);
    const steps = script.order.map(k => ({ key: k, label: PD_NODES[k].title, state: 'pending' }));
    let prog = 0; const total = script.dur.reduce((a, b) => a + b, 0);
    const state = { label, note: '准备中…', progress: 0, status: 'running', steps: steps.map(s => ({ ...s })), doneKeys: [], activeKey: null };
    runRef.current = { interrupted: false };
    setRun({ ...state });

    for (let i = 0; i < script.order.length; i++) {
      if (runRef.current.interrupted) {
        state.status = 'interrupted'; state.note = '已被用户中断';
        state.steps = state.steps.map((s, j) => j >= i ? { ...s, state: s.state === 'active' ? 'pending' : s.state } : s);
        setRun({ ...state, activeKey: null });
        setRuns(prev => [{ label, version: verLabel || versions[curIdx].label, status: 'interrupted' }, ...prev]);
        setBusy(false); return;
      }
      const k = script.order[i];
      state.activeKey = k;
      state.steps[i].state = 'active';
      state.note = `执行 ${PD_NODES[k].title}…`;
      setRun({ ...state });
      await wait(script.dur[i]);
      prog += script.dur[i];
      state.progress = prog / total;

      if (script.fail && script.fail.key === k) {
        state.steps[i].state = 'failed'; state.activeKey = null; state.status = 'failed';
        state.note = '执行失败 — 见对话';
        setRun({ ...state });
        setErrorKey(k); setSelected(k);
        setRuns(prev => [{ label, version: verLabel || versions[curIdx].label, status: 'failed' }, ...prev]);
        // agent surfaces error + fix proposal
        await wait(500);
        pushMsg({ role: 'agent', text: '运行在 Load ControlNet 处中断了。我已定位原因：', error: script.fail.human ? script.fail : null });
        await agentThink(['正在扫描 models/controlnet 目录…', '正在匹配可用的 depth 模型…'],
          { role: 'agent', text: '找到一个兼容模型，建议做最小修复（只改一个参数，不动其它节点）：', proposal: PROPOSAL_FIX, propState: 'pending' });
        setPending(PROPOSAL_FIX); setErrorKey(k);
        setBusy(false); return;
      }
      state.steps[i].state = 'done'; state.doneKeys.push(k); state.activeKey = null;
      setRun({ ...state });
    }
    state.status = 'succeeded'; state.note = '完成 · 1 张输出'; state.progress = 1;
    setRun({ ...state });
    setRuns(prev => [{ label, version: verLabel || versions[curIdx].label, status: 'succeeded' }, ...prev]);
    const hue = 268;
    setOutputs([{ id: 'o' + Date.now(), seed: '843 629', hue, sel: true }]);
    setBusy(false);
  };

  // ---- seed sweep run (after confirm) ----
  const doSweep = async () => {
    setBusy(true); setHistoryOpen(false); setErrorKey(null);
    const outs = [];
    for (let s = 0; s < 4; s++) {
      const label = `Seed 试验 ${s + 1}/4`;
      const steps = RUN_V1.order.map(k => ({ key: k, label: PD_NODES[k].title, state: 'pending' }));
      const state = { label, note: `seed ${SWEEP_SEEDS[s]}`, progress: 0, status: 'running', steps, doneKeys: [], activeKey: null };
      const total = RUN_V1.dur.reduce((a, b) => a + b, 0); let prog = 0;
      for (let i = 0; i < RUN_V1.order.length; i++) {
        if (runRef.current && runRef.current.interrupted) { setBusy(false); return; }
        const k = RUN_V1.order[i]; state.activeKey = k; state.steps[i].state = 'active';
        setRun({ ...state, steps: state.steps.map(x => ({ ...x })) });
        await wait(i === 4 ? 520 : 150);
        prog += RUN_V1.dur[i]; state.progress = prog / total;
        state.steps[i].state = 'done'; state.doneKeys.push(k); state.activeKey = null;
      }
      outs.push({ id: 'sw' + s, seed: SWEEP_SEEDS[s], hue: SWEEP_HUES[s], sel: false });
      setOutputs(outs.map((o, i) => ({ ...o, sel: i === s })));
    }
    const best = 2;
    setRun({ label: 'Seed 试验 · 完成', note: '4 张输出', progress: 1, status: 'succeeded', steps: [], doneKeys: [], activeKey: null });
    setOutputs(outs.map((o, i) => ({ ...o, sel: i === best })));
    setRuns(prev => [{ label: 'Seed 试验 ×4', version: versions[curIdx].label, status: 'succeeded' }, ...prev]);
    pushMsg({ role: 'agent', text: 'seed 试验完成。', sweep: { best } });
    setBusy(false);
  };

  // ---- top bar actions ----
  const onQueue = () => {
    runRef.current = { interrupted: false };
    // if controlnet graph but unfixed model -> fail; else success
    const isCN = !!graph.nodes.cnl;
    const fixed = isCN && graph.nodes.cnl.params[0].v.includes('cn-depth-sdxl');
    if (isCN && !fixed) doRun(RUN_V2_FAIL, '运行 Queue · 含 ControlNet');
    else if (isCN && fixed) doRun(RUN_V3, '运行 Queue · ControlNet depth');
    else doRun(RUN_V1, '运行 Queue · 文生图');
  };
  const onInterrupt = () => { if (runRef.current) runRef.current.interrupted = true; };

  const onAgentRun = () => {
    if (busy) return;
    setConfirm({ versionLabel: versions[curIdx].label, runs: 4, pending: !!pending, uploads: graph.nodes.img ? 'ref_depth.png' : '无', kind: 'sweep' });
  };
  const confirmRun = () => {
    const plan = confirm; setConfirm(null);
    if (pending) { setPending(null); }
    runRef.current = { interrupted: false };
    if (plan.kind === 'sweep') doSweep();
  };

  const requestRun = (msgId) => {
    patchMsg(msgId, { runplanState: 'confirmed' });
    setConfirm({ versionLabel: versions[curIdx].label, runs: 4, pending: !!pending, uploads: '无', kind: 'sweep' });
  };

  // ---- undo / restore ----
  const onUndo = () => {
    if (curIdx === 0) return;
    setCurIdx(curIdx - 1); setPending(null); setErrorKey(null);
    pushMsg({ role: 'sys', text: `已撤销 · 回到 ${versions[curIdx - 1].label}` });
  };
  const onRestore = (idx) => {
    const restored = versions[idx];
    setVersions(prev => [...prev, { graph: restored.graph, label: restored.label, source: 'restore' }]);
    setCurIdx(versions.length); setPending(null); setErrorKey(null);
    pushMsg({ role: 'sys', text: `已恢复版本 v${idx} · ${restored.label}` });
  };

  // ---- fix demo: ensure a broken-controlnet graph exists, then run (it will fail → agent proposes fix) ----
  const flowFixDemo = async () => {
    setBusy(true); setHistoryOpen(false);
    // set up a controlnet graph with the broken model if not already present
    if (!graph.nodes.cnl) {
      pushMsg({ role: 'user', text: '加载我上次那个 ControlNet 工作流，跑一下。' });
      await agentThink(['正在恢复 ControlNet depth 工作流…'], { role: 'agent', text: '已载入含 ControlNet 的工作流，开始运行：' });
      commitVersion(GRAPH_V2, '+ ControlNet Depth', 'agent');
      await wait(450);
    }
    setBusy(false);
    runRef.current = { interrupted: false };
    doRun(RUN_V2_FAIL, '运行 Queue · 含 ControlNet', '+ ControlNet Depth');
  };

  // ---- preset / send routing ----
  const onPreset = (id) => {
    if (busy) return;
    setHistoryOpen(false);
    if (id === 'create') flowCreate();
    else if (id === 'controlnet') flowControlNet();
    else if (id === 'sweep') flowSweep();
    else if (id === 'fix') flowFixDemo();
  };
  const onSend = (text) => {
    setHistoryOpen(false);
    const t = text.toLowerCase();
    if (/controlnet|control net|深度|depth|姿态|构图控制/.test(t)) flowControlNet(text);
    else if (/seed|试验|sweep|多.*跑|批量|挑.*张/.test(t)) flowSweep(text);
    else if (/修复|报错|错误|fix|失败/.test(t)) flowFixDemo();
    else flowCreate(text);
  };

  const queueDisabled = busy || Object.keys(graph.nodes).length === 0 || !!pending;
  const queueHint = pending ? '请先处理待确认的图变更' : Object.keys(graph.nodes).length === 0 ? '画布为空' : '';

  return (
    <div className="wb">
      <PTopBar
        canUndo={curIdx > 0 && !busy} onUndo={onUndo}
        onHistory={() => setHistoryOpen(o => !o)} historyOpen={historyOpen}
        onQueue={onQueue} queueDisabled={queueDisabled} queueHint={queueHint}
        onAgentRun={onAgentRun} running={run && run.status === 'running'} onInterrupt={onInterrupt} />
      <div className="wb-body">
        <div className="wb-chat" style={{ flex: '0 0 37%', minWidth: 360 }}>
          <ChatPane messages={messages} presets={PRESETS} onPreset={onPreset} onSend={onSend}
            onApply={applyProposal} onDismiss={dismissProposal} onRequestRun={requestRun} busy={busy} />
        </div>
        <div className="wb-canvas" style={{ display: 'flex', flexDirection: 'column', position: 'relative' }}>
          <div style={{ position: 'relative', flex: 1, minHeight: 0 }}>
            <ProtoCanvas graph={graph} pending={pending} run={run && run.status === 'running' ? run : (run && (run.activeKey || run.doneKeys.length) ? run : null)}
              errorKey={errorKey} selected={selected} onSelect={setSelected} styleCls="cv-bold" />
            <PHistory open={historyOpen} versions={versions} currentIdx={curIdx} runs={runs} onRestore={onRestore} onClose={() => setHistoryOpen(false)} />
            <PConfirm plan={confirm} onConfirm={confirmRun} onCancel={() => setConfirm(null)} />
          </div>
          <PRunDock run={run} />
          <POutputs outputs={outputs} onSelect={(id) => setOutputs(prev => prev.map(o => ({ ...o, sel: o.id === id })))} />
        </div>
      </div>
    </div>
  );
}

Object.assign(window, { ProtoApp });
