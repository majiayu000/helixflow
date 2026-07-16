# Product Spec

## Linked Issue

GH-118（https://github.com/majiayu000/helixflow/issues/118）

complexity: large

## 用户问题

布局、手动 graph ops 与 proposal 应用会先写文件，再提交 SQLite version/CAS。当前布局文件名由 base version 派生，同一 base 的并发请求会覆盖同一个目标；数据库失败或冲突后，失败请求可能改变成功版本实际读取到的内容。数据库虽保存 `graph_hash`，读取时却没有校验，因此缺失、截断或被覆盖的文件仍可能作为已确认版本返回、运行或回退。

## 目标

- 每个逻辑候选版本使用唯一、不可变的文件身份；重试只能复用内容完全相同的既有候选。
- 文件先经临时文件完整写入并原子发布，再由单个数据库 transaction/CAS 引用。
- 失败请求只清理自己拥有且经数据库确认未被引用的文件。
- 所有 version graph 读取强制校验已存 `graph_hash`，缺失、损坏或错配时显式失败。
- 启动时对账 DB/file 引用，并以可审计结果处理已引用损坏与可安全识别的孤儿候选。
- 用并发与 fault-injection 测试证明 file/DB 各阶段失败不会产生污染或半提交。

## 非目标

- 不改变 graph op、layout、proposal、undo/restore 的业务语义或 API wire shape。
- 不引入跨进程分布式事务、CRDT、对象存储或数据库 schema migration。
- 不负责 Canvas comments 的 `base_seq`、operation id、SQLite CAS 或 comments JSON 迁移；这些完全属于 GH-120。
- 不实现通用历史版本删除、保留期或任意遗留文件垃圾回收。

## Behavior Invariants

1. **B-001** 合法 graph/layout/proposal 版本写入成功后，数据库中的 `graph_path` 必须指向一个已完整发布的不可变文件，且文件实际 SHA-256 必须精确等于该 version 的 `graph_hash`。
2. **B-002** 每个新逻辑候选使用唯一文件身份；不得再按共享 base version 覆盖目标。带 `idempotencyKey` 的重试只有在 workspace、base、候选内容 hash 与已提交 version 全部匹配时才可返回原成功结果，否则明确冲突且不得覆盖文件。
3. **B-003** 候选内容必须先写入本请求独占的 temp 文件，再以同一文件系统内的原子 rename 发布；temp 写入、flush 或 rename 失败时不得创建 version，也不得把部分内容暴露为 final 文件。
4. **B-004** 文件完整发布后才能进入数据库 transaction/CAS；transaction 必须原子完成 version 插入、workspace current 指针推进，以及该路径原有的 proposal/message 状态更新，任一步失败均不得留下数据库半状态。
5. **B-005** 两个基于同一 current version 的并发写入至多一个成功；失败者不得覆盖、删除或改变成功者的候选文件、version、proposal/message 状态或 current 指针。
6. **B-006** CAS/数据库失败后，只能删除本请求创建且数据库再次确认未被任何 version/proposal 引用的 temp/final 文件；引用检查失败或提交结果不明确时必须保留文件并显式报错，不能猜测后删除。
7. **B-007** 文件阶段失败时数据库保持不变；数据库阶段失败时，清理成功或保留原因必须可观测，清理错误不得被静默吞掉或伪装成成功。
8. **B-008** 读取任一 version graph（workspace state、canvas、run、layout/ops/proposal base、export）时必须在反序列化前后校验实际字节 hash 与 `graph_hash`；文件缺失、非法 JSON 或 hash 错配均返回明确 server error，不得回退到空图、旧图或未校验内容。
9. **B-009** undo/restore 在创建引用目标文件的新 restore version 前必须验证目标文件与目标 `graph_hash`；损坏目标不得推进 current version。
10. **B-010** 服务启动对账必须覆盖所有 DB-referenced version graph，以及 proposal 记录引用的 ops/preview 文件；已引用文件缺失、非法或 version hash 错配时服务 fail closed，不开始监听请求。
11. **B-011** 启动对账只能自动清理命名规则可证明属于本机制、且数据库确认无引用的 temp/候选文件；未知遗留文件只报告不删除。对账输出必须包含 verified、removed、retained、corrupt 的结构化计数与路径类别。
12. **B-012** 崩溃发生在 temp 写入、atomic rename 或 DB commit 任一边界时，重启对账后不得出现“DB 已引用但文件缺失/错配仍继续服务”；无引用候选可安全清理或明确保留。
13. **B-013** 存量 version 路径无需迁移；只要路径安全、文件存在、JSON 合法且 hash 匹配，继续可读。没有可信 `sha256:` hash 的记录按损坏处理，不默认放行。
14. **B-014** 初始 workspace graph、layout、manual ops、人工 proposal apply 与 agent auto-apply 都遵守同一文件发布和失败清理契约；任何入口不得保留覆盖式特例。
15. **B-015** comments、presence 与 WebSocket 协作行为保持兼容；GH-118 不读取、写入、迁移或清理 `comments.json`，也不改变 GH-120 的 sequence/CAS 真相源。

## 验收标准

- [ ] 同一 base 并发布局/ops/proposal 候选时至多一个 version 成功，成功文件内容与 DB hash 精确匹配，失败候选不污染成功结果。
- [ ] temp write、rename、DB insert、CAS、commit 后结果不明确等 fault point 均有确定性负例，断言 DB/current/file 引用没有半状态。
- [ ] DB/CAS 失败仅清理本请求未引用候选；引用检查失败时文件保留且错误可见。
- [ ] workspace、canvas、run、export 与 restore 对缺失、非法 JSON、hash mismatch 都 fail closed。
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

无 API 或 schema migration。部署时首次启动会执行严格对账；若存量 DB 指向缺失、非法或 hash 不匹配的 graph 文件，服务将拒绝启动并给出结构化错误，需运维从可信备份恢复或显式修复后再启动。GH-120 合并后实现必须重新基于最新 `origin/main`，确认 comments 持久化改动未进入本契约。
