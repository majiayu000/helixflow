# Product Spec

## Linked Issue

GH-118（https://github.com/majiayu000/helixflow/issues/118）

complexity: large

## 用户问题

布局、手动 graph ops 与 proposal 应用会先写文件，再提交 SQLite version/CAS。当前布局文件名由 base version 派生，同一 base 的并发请求会覆盖同一个目标；数据库失败或冲突后，失败请求可能改变成功版本实际读取到的内容。数据库虽保存 `graph_hash`，读取时却没有校验，因此缺失、截断或被覆盖的文件仍可能作为已确认版本返回、运行或回退。初始 workspace 还会先创建 workspace 行、再写文件和 version，人工 proposal apply 则在 version transaction 提交后才创建 applied message，两者都可能留下数据库半状态。消息入口还把客户端提交的 graph 直接交给 agent/run，绕过当前 `VersionRecord` 的文件校验。

## 目标

- 每个逻辑候选版本使用唯一、不可变的文件身份；重试只能复用内容完全相同的既有候选。
- 文件先经临时文件完整写入并原子发布，再由单个数据库 transaction/CAS 引用。
- 失败请求只清理自己拥有且经数据库确认未被引用的文件。
- 所有 version graph 读取强制校验已存 `graph_hash`，缺失、损坏或错配时显式失败。
- 启动时对账 DB/file 引用，并以可审计结果处理已引用损坏与可安全识别的孤儿候选。
- 用并发与 fault-injection 测试证明 file/DB 各阶段失败不会产生污染或半提交。
- 明确唯一状态所有者：SQLite `VersionRecord` 与 workspace current pointer 是提交真相；文件候选生命周期只是受该真相约束的存储 adapter，经 hash verified read 与启动对账闭环，绝不成为第二真相源。

## 非目标

- 不改变 graph op、layout、proposal、undo/restore 的业务语义或 API wire shape。
- 不引入新 crate、跨进程分布式事务、CRDT、对象存储或数据库 schema migration；本变更是现有 Store/server/filesystem 边界接线补全。
- 不负责 Canvas comments 的 `base_seq`、operation id、SQLite CAS 或 comments JSON 迁移；这些完全属于 GH-120。
- 不实现通用历史版本删除、保留期或任意遗留文件垃圾回收。

## Behavior Invariants

1. **B-001** 合法 graph/layout/proposal 版本写入成功后，数据库中的 `graph_path` 必须指向一个已完整发布的不可变文件，且文件实际 SHA-256 必须精确等于该 version 的 `graph_hash`。
2. **B-002** 每个新逻辑候选使用唯一文件身份；不得再按共享 base version 覆盖目标。无 key 请求使用 UUID；带 `idempotencyKey` 的 ops 请求用 workspace、base 与 key 的域分隔 opaque digest 派生稳定身份，raw key 不得进入路径。重试只有在该逻辑身份、workspace、base、候选内容 hash 与已提交 version 全部匹配时才可返回原成功结果；同 key 不同内容或不同 key 相同内容都不得靠内容扫描误判为 replay。
3. **B-003** 候选内容必须先写入与 final 同目录、本请求独占的 temp 文件，完整写入并 `sync_all` 后，再用同一文件系统内的 exclusive no-replace 原子发布；普通可覆盖目标的 rename 被禁止。temp 写入、flush、publish 或目录同步失败时不得创建 version，也不得把部分内容暴露为 final 文件。
4. **B-004** 文件完整发布后才能进入数据库 transaction/CAS；transaction 必须原子完成 version 插入、workspace current 指针推进，以及该路径原有的 proposal 状态和 message 创建/状态更新。当前人工 proposal apply 会创建 `proposal_applied` message，因此该 message 必须与 version/current/proposal 状态在同一个 Store transaction 内提交，任一步失败均不得留下数据库半状态。
5. **B-005** 两个基于同一 current version 的并发写入至多一个成功；失败者不得覆盖、删除或改变成功者的候选文件、version、proposal/message 状态或 current 指针。
6. **B-006** CAS/数据库失败后，只能删除本请求创建且数据库再次确认未被任何 version/proposal 引用的 temp/final 文件；引用检查失败或提交结果不明确时必须保留文件并显式报错，不能猜测后删除。
7. **B-007** 文件阶段失败时数据库保持不变；数据库阶段失败时，清理成功或保留原因必须可观测，清理错误不得被静默吞掉或伪装成成功。
8. **B-008** 读取任一 version graph（workspace state、canvas、直接 run、workbench message 的 chat/proposal/run/sweep、layout/ops/proposal base、export）时必须按当前 `VersionRecord` 校验实际字节 hash 与 `graph_hash` 后才反序列化和消费；文件缺失、非法 JSON 或 hash 错配均返回明确 server error，不得回退到空图、旧图或客户端 graph。消息 API wire shape 保持不变，但客户端 graph 只用于与服务器 current graph 做一致性校验/上下文，agent 与 run 实际消费服务器 verified graph。
9. **B-009** undo/restore 在创建引用目标文件的新 restore version 前必须验证目标文件与目标 `graph_hash`；损坏目标不得推进 current version。
10. **B-010** 服务启动对账必须覆盖所有 DB-referenced version graph，以及 proposal 记录引用的 ops/preview 文件；已引用文件缺失、非法或 version hash 错配时服务 fail closed，不开始监听请求。
11. **B-011** 启动对账只能自动清理命名规则可证明属于本机制、且数据库确认无引用的 temp/候选文件；未知遗留文件只报告不删除。成功报告必须以结构化值保留在 `AppState` 并在开始监听前输出结构化日志，包含 verified、removed、retained、corrupt 的计数与路径类别；失败必须是结构化、脱敏错误，不泄漏 data-dir 绝对路径、文件内容或 SQL 细节。
12. **B-012** 崩溃发生在 temp 写入、exclusive no-replace publish 或 DB commit 任一边界时，重启对账后不得出现“DB 已引用但文件缺失/错配仍继续服务”；无引用候选可安全清理或明确保留。
13. **B-013** 存量 version 路径无需迁移；只要路径安全、文件存在、JSON 合法且 hash 匹配，继续可读。没有可信 `sha256:` hash 的记录按损坏处理，不默认放行。
14. **B-014** 初始 workspace graph、layout、manual ops、人工 proposal apply 与 agent auto-apply 都遵守同一文件发布和失败清理契约；任何入口不得保留覆盖式特例。初始 workspace 必须先无副作用地预分配 workspace/version identity，用 workspace ID 构造并发布候选路径，再由一个 Store transaction 插入 workspace、initial version 和 current pointer；transaction 失败或结果不明确时不得出现只创建 workspace 的数据库状态。
15. **B-015** comments、presence 与 WebSocket 协作行为保持兼容；GH-118 不读取、写入、迁移或清理 `comments.json`，也不改变 GH-120 的 sequence/CAS 真相源。

## 验收标准

- [ ] 同一 base 并发布局/ops/proposal 候选时至多一个 version 成功，成功文件内容与 DB hash 精确匹配，失败候选不污染成功结果。
- [ ] temp write、exclusive no-replace publish、DB insert、CAS、commit 后结果不明确等 fault point 均有确定性负例，断言 DB/current/file 引用没有半状态。
- [ ] DB/CAS 失败仅清理本请求未引用候选；引用检查失败时文件保留且错误可见。
- [ ] workspace、canvas、run、export 与 restore 对缺失、非法 JSON、hash mismatch 都 fail closed。
- [ ] workbench message 的 chat/proposal/run/sweep 不再信任客户端 graph；base/current 不一致或服务器 version 文件损坏时，agent/run 不启动且不创建后续业务记录。
- [ ] 初始 workspace 的 workspace/version/current 三者在一个 transaction 中提交；人工 proposal 的 version/current/proposal/applied message 四者在一个 transaction 中提交，fault point 均无半状态。
- [ ] 启动对账可检测已引用损坏，并仅清理可证明无引用的本机制候选，输出结构化报告。
- [ ] 既有 API、idempotent retry、proposal transaction、undo/restore 行为与 GH-120 comments 行为不回退。

## 边界情况

| 类别 | 判定 |
| --- | --- |
| Empty / missing input | covered: B-008, B-010, B-013 |
| Error and failure paths | covered: B-003, B-004, B-006, B-007 |
| Authorization / permission | N/A：本 issue 不新增授权面；文件系统权限失败属于 B-003/B-007 |
| Concurrency / race / ordering | covered: B-004, B-005, B-006 |
| Retry / repetition / idempotency | covered: B-002, B-012 |
| Illegal state transitions | covered: B-004, B-005, B-009 |
| Compatibility / migration | covered: B-013, B-014, B-015 |
| Degradation / fallback | covered: B-007, B-008, B-010 |
| Evidence and audit integrity | covered: B-001, B-011 |
| Cancellation / interruption / partial completion | covered: B-003, B-012 |

## 发布说明

无 API wire、schema 或 crate migration。部署时首次启动会执行严格对账；若存量 DB 指向缺失、非法或 hash 不匹配的 graph 文件，服务将拒绝启动并给出结构化、脱敏错误，需运维从可信备份恢复或显式修复后再启动。合法 legacy graph path 立即收敛到 verified-read 契约但不强制改名；proposal ops/preview 立即收敛到安全 JSON 读取契约。GH-120 已合并，实施基线必须是包含 PR #122 的最新 `origin/main`，且不得修改 comments 持久化文件。
