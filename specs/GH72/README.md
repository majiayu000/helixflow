# Helixflow Workbench UI Refresh

## 状态

- SpecRail route: `write_spec`
- Locale: `zh-CN`
- Linked issue: GH72
- Gate evidence: `python3 checks/route_gate.py --repo . --route write_spec --issue 72 --evidence artifacts/specrail/issue-72.json --json` returned `allowed`.
- Source design: `/Users/lifcc/Downloads/HelixFlow UI设计.zip`
- Persisted evidence:
  - `artifacts/ui-design/helixflow-workbench-20260702/Helixflow-Workbench.dc.html`
  - `artifacts/ui-design/helixflow-workbench-20260702/support.js`
  - `artifacts/ui-design/helixflow-workbench-20260702/design-full.png`

## 结论

这份 UI 设计不应该按“换主题皮肤”应用。它定义的是一个新的 Workbench 产品循环：

1. 用户先在真实 workspace 上审阅 Agent proposal。
2. 用户可以进入手动编辑态，编辑累积为 uncommitted ops。
3. 未提交编辑会锁住 Queue，必须先提交为新版本，例如 `v5 source=user`。
4. Agent 后续 proposal 只能基于最新已提交版本。
5. 运行、花费、失败诊断、输出选择都留在同一个导演工作台里闭环。

推荐采用设计稿里的 `2a` 导演工作台作为主壳，叠加 `3a` 手动编辑能力；`2b`、`2c`、`2d`、`2e` 作为运行确认、输出评审、失败修复、模板起点状态。`1a`、`1b`、`1c` 视为探索方向，不作为第一轮实现目标。

## 相关现有工作

- 远端 open issues 中没有完整 Workbench UI refresh issue。
- 相关但不等价的 issue:
  - `#64` Inspector 参数内联编辑
  - `#66` 节点库面板 + 画布增删节点 + 复制粘贴闭环
  - `#59` 统一图编辑 op 模型
- 本地 `specs/canvas-agent-full/` 已经定义 canvas source-of-truth 和 durable `canvas_ops` 的后续方向。本 packet 不替代它，而是把新 UI 设计映射到可实现的产品壳与交互顺序。

## 文件

- `analysis.md`: 设计稿到现有代码的差距分析和应用策略。
- `product.md`: 用户可见行为契约。
- `tech.md`: 技术落地设计、数据流、风险和测试映射。
- `tasks.md`: 分步任务、并行拆分、验证和 handoff。

## Human Gate

本 packet 已绑定 `GH72`。进入实现前仍需要维护者完成 SpecRail human gates：

1. 确认 `GH72` 是本次 Workbench UI refresh 的 umbrella issue。
2. 审阅并批准 `product.md`、`tech.md`、`tasks.md`。
3. 将 issue 从 `ready_to_spec` 推进到 `ready_to_implement` 后，才能按 `implx` 进入实现 tranche。

在 `ready_to_implement` / spec approval 前，本 packet 只能作为已绑定 issue 的设计草案，不能声称实现可直接开始。
