# Product Spec

## Linked Issue

GH-102 (#102)

关联决策：本 spec 显式调整 GH-91 保留的人工 proposal 审批流程；GH-101 的自愈重跑与产物审查依赖本 spec 提供的统一成本闸门和起跑契约。

## 用户问题

用户让 agent 修改 workflow 后，当前主流程需要先人工批准 proposal，再单独处理 run 成本确认。连续的人工停顿打断了“描述需求 → 形成版本 → 安全起跑”的体验，也使不同 run 入口容易形成不一致的成本判断。

直接取消审批还会带来一致性风险：proposal 应用、版本切换或起跑失败时，如果系统留下半完成状态，后续 agent 请求可能被旧的 pending proposal 阻塞，用户也难以判断 workspace 当前到底是哪一版。

## 目标

- 合法的 agent proposal 自动应用为可追踪、可回退的新 workflow version。
- 自动应用是原子的用户体验：失败时不改变 current version，也不留下阻塞后续操作的 pending proposal。
- agent 发起普通 run request 或 seed sweep 时统一估算成本；安全阈值内自动起跑，超过阈值必须等待用户确认。
- 自动起跑和确认后起跑具有一致的状态、事件、成本记录和错误可见性。
- 并发 proposal 不覆盖较新的 workspace 版本。

## 非目标

- 不实现 run 执行失败后的自动重试、retry lineage 或 artifact accept/reject；这些属于 GH-101。
- 不改变 graph op、provider 或 sweep recommendation 的业务语义。
- 不引入新的计费方式、供应商或支付流程。
- 不承诺自动修复无效 proposal；无效输入仍应明确失败。

## Behavior Invariants

1. 通过校验且基于 current version 的 agent proposal 自动产生一个新 version；成功后 proposal 为 applied，workspace current version 指向该新 version。
2. 自动应用成功后，用户能看到 proposal 已应用的消息、目标 version，并能通过既有 version history 回退。
3. proposal 在校验、持久化、并发校验或版本创建任一步失败时，workspace current version 保持不变，且不得遗留会阻塞后续请求的 pending proposal。
4. 同一 workspace 的并发 proposal 只能应用仍基于 current version 的请求；过期请求明确返回冲突，不覆盖先完成的新版本。
5. 自动应用 proposal 本身不隐式创建 run；agent 发起普通 run request 或 seed sweep 时必须先获得可解释的成本估算，再决定是否起跑，估算失败时不得启动 run。
6. 普通 run request 或 seed sweep 的估算总成本不高于配置阈值时自动启动；高于阈值时保持 `waiting_confirmation`，用户确认前不得调用 provider。
7. 用户确认等待中的 run 后，其执行入口、run events 和成本 ledger 语义与阈值内自动起跑一致。
8. 应用、估算或后台启动失败必须以错误状态和用户可见信息呈现，不得仅记录 warning 后静默降级。
9. 阈值配置缺失时采用文档化的保守默认值；无效配置明确报错或采用同样保守且可观测的行为。
10. 既有人工 graph 编辑、version history、undo/restore 与非 agent run 路径保持兼容。

## 验收标准

- [ ] 合法 agent proposal 自动形成 applied proposal 和新 current version，并产生包含 version 引用的用户消息。
- [ ] 人为制造 proposal 应用失败后，current version 未变化，且下一次合法 agent 请求不会被遗留 pending proposal 阻塞。
- [ ] 两个基于同一旧版本的 proposal 并发提交时，至多一个成功，另一个返回明确冲突。
- [ ] agent 普通 run request 与 seed sweep 在阈值内自动起跑；超阈值只进入 `waiting_confirmation`，确认后才调用 provider。
- [ ] 两条起跑路径都写入 estimated/actual cost ledger 并产生一致的 run events；失败可见。
- [ ] 既有 version 回退、人工编辑和 Web 主流程回归测试通过。

## 边界情况

- proposal 文件已经写入但数据库应用失败时，允许保留不被引用的诊断文件，但不得保留阻塞性 pending 状态。
- 自动应用成功后不会隐式创建 run；后续独立 run request 估算失败时，新 version 保持不变、run 不启动，用户收到明确错误并可继续编辑或重试。
- 用户在等待确认期间切换 version 时，确认动作必须按既有 run/version 绑定执行，不能隐式改跑新版本。
- 阈值恰好相等时视为阈值内。
- 重复确认、网络重试或并发确认不得导致同一个 run 被启动两次。

## 发布说明

- 用户可见变化：agent proposal 不再要求单独审批；应用成功后直接形成可回退版本，并按成本阈值自动起跑或等待确认。
- 部署文档必须说明确认阈值环境变量、默认值和无效配置行为。
- GH-101 在本 spec 合并后实现 run 失败自愈和输出审查。
