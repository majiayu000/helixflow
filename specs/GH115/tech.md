# Tech Spec

## Linked Issue

GH-115：<https://github.com/majiayu000/helixflow/issues/115>

## Product Spec

[`product.md`](product.md)

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Provider registry | `crates/gateway/src/registry.rs` | `from_env()` 缺少 `HELIXFLOW_RUNTIME_PROVIDER` 时回退 `mock`，且恒注册 mock | P1/P2 的直接根因 |
| Mock provider/catalog | `crates/gateway/src/lib.rs`, `crates/gateway/src/runtime_provider.rs` | image 只有 8-byte PNG signature，video 是普通文本，却声明 `image/png` / `video/mp4`；catalog 仅写 local test | P3/P7 与伪媒体 success 根因 |
| Artifact persistence | `crates/run/src/artifacts.rs`, `crates/run/src/executor.rs` | inline/remote bytes 直接写文件，之后创建 DB artifact；没有 MIME/kind/content validation | P4/P5/P6 的执行断点 |
| Server workspace defaults | `crates/server/src/app_state.rs`, `crates/server/src/workspace_state.rs` | workspace 没有持久化选择时继承 registry default，因此生产默认 mock | P1 的用户可见传播路径 |
| Rust tests | gateway/run/server 对应测试 | 多数测试显式注入 mock；缺少 env 装配与 invalid media 失败断言 | 需要锁定 fail-closed 与状态可信性 |

## 设计方案

### 1. Provider registry fail closed

- 新增稳定 unavailable id `unconfigured`，作为缺少/空白
  `HELIXFLOW_RUNTIME_PROVIDER` 时的 default。
- 新增显式布尔开关 `HELIXFLOW_ENABLE_MOCK_PROVIDER`。仅当值为 `1`、`true`、`yes`
  或 `on`（ASCII 大小写不敏感）时注册 `RuntimeProvider::mock()`；缺失、空白或未知值均关闭。
- 即使 `HELIXFLOW_RUNTIME_PROVIDER=mock`，开关未启用时也通过现有
  `RuntimeProvider::unavailable` 合成 unavailable default，不回退到 Atlas/FAL 或 mock。
- Atlas/FAL 继续按现有 env 配置注册 enabled/unavailable 条目。`ProviderRegistry::new()`
  继续支持测试显式注入，不把环境读取混进构造器。
- `selected_provider()` 保持“持久化选择优先，否则 registry default”；因此新 workspace
  自动继承 `unconfigured` unavailable 状态，server 无需伪造 workspace provider 字段。

### 2. Mock 生成合法的确定性测试媒体

- 保留 `MockProvider` 的 text/image/video capabilities，但把 image/video 内容替换为固定、
  小型、可由同一 production validator 接受的 synthetic fixtures。
- catalog label/kind 保持 `Mock (local test)` / `local_test`，health/catalog message 改成明确
  `non-production synthetic` 文案。
- fixture 不含用户输入、secret、网络 URL；同类请求输出 bytes 固定，metadata 继续标注
  `provider=mock` 与 `deterministic=true`。

### 3. 写入前媒体校验

- 在 `persist_provider_artifact()` 内、`write_artifact_file()` 前统一执行
  `validate_artifact_bytes(payload, bytes)`；inline 与 HTTPS 下载使用同一路径。
- 先验证 kind/MIME 契约：`ArtifactKind::Image + image/png`、
  `ArtifactKind::Video + video/mp4` 才进入对应 validator；声明媒体但没有可校验 bytes 时
  fail closed。
- PNG validator：检查 signature、首个 `IHDR`（长度 13、正 dimensions）、完整 chunk
  边界、至少一个非空 `IDAT`、零长度 `IEND` 且无截断/尾随结构错误。
- MP4 validator：以 checked arithmetic 遍历 ISO BMFF box，支持普通 32-bit size 与
  64-bit extended size；要求有效首盒 `ftyp`、合法 compatible brand、非空 `moov`、非空
  `mdat`，拒绝 size 越界、零长度截断和普通文本。
- 校验错误统一为 `RunError::ArtifactPersistence`，消息只含媒体类型与安全原因，不拼接
  URL、bytes 或绝对路径。
- validator 成功后才写文件并创建 DB artifact；executor 已有的错误路径会把当前 step/run
  更新为 `failed` 并阻止 `run.succeeded`。

### 4. 测试策略与环境隔离

- gateway 单元测试使用不依赖进程级 env 的 `from_config`/私有解析 helper 验证 P1/P2，
  避免并行测试修改全局环境变量产生竞态。
- run focused tests 增加 invalid PNG、文本 MP4 的 persistence 拒绝，以及返回 invalid media
  的 provider 导致 step/run failed、无 artifact、无 success event。
- 保留显式 `ProviderRegistry::new("mock", ...)` 与 server test helpers 的 mock 注入；这些
  是明确测试配置，不改变生产 `from_env()` 语义。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | `gateway/registry.rs`, `server/app_state.rs`, workspace state tests | missing/blank config snapshot default=`unconfigured`, enabled=false；provider run unavailable |
| P2 | `gateway/registry.rs` | table-driven config tests 覆盖两项配置单独/同时设置 |
| P3 | `gateway/runtime_provider.rs`, gateway tests | catalog label/kind/message 精确断言 non-production synthetic |
| P4 | `run/artifacts.rs`, run tests | header-only/truncated PNG 被拒；valid mock PNG 被接受 |
| P5 | `run/artifacts.rs`, run tests | text/truncated MP4 被拒；valid mock MP4 被接受 |
| P6 | `run/executor.rs` 既有错误流 + run integration test | step/run=`failed`、artifacts empty、无 `run.succeeded` |
| P7 | `gateway/lib.rs`, run existing tests | mock deterministic bytes；manual run 继续 succeeded |
| P8 | shared persistence path | workspace tests + `cargo test --workspace` 回归 |

## 数据流

1. server 启动读取 provider/default 与 mock 开关，构造 `ProviderRegistry`。
2. 无配置时 registry 合成 `unconfigured` unavailable；workspace state 把它作为
   `defaultProvider` / `selectedProvider` 返回。
3. run executor 调用显式选择的 provider；unavailable provider 直接返回错误。
4. provider 返回 artifact payload 后，persistence 下载/读取 bytes，按 kind+MIME 校验。
5. 只有校验成功才落盘、计算 hash、写 artifacts 表并把 step/run 标记成功；任一错误沿
   `RunResult` 返回并进入现有失败状态机。

## 备选方案

- **仅把默认值从 mock 改为 Atlas/FAL：拒绝。** 仍会在缺少 secret 时产生模糊默认，且
  不能解决 invalid artifact success。
- **保留假媒体但加 UI badge：拒绝。** 不能修复后端状态真实性，且本 tranche 无 `web/**`
  写权限。
- **只检查 magic bytes/扩展名：拒绝。** 现有 8-byte PNG 正是 magic-only 反例；至少需要
  完整基本容器结构。
- **引入外部 ffmpeg/image decoder：暂不采用。** 会增加运行时系统依赖或较大 Rust
  依赖；当前结构 validator 足以 fail closed 已知伪媒体并可确定性测试。后续可增强为完整
  decode probe，但不得弱化本 spec 的失败契约。

## 风险

- Security: box/chunk 解析必须使用长度上限和 checked arithmetic，避免恶意 bytes 导致
  panic/overflow；错误不得回显远端 URL 或 secret。
- Compatibility: 未配置 provider 的本地启动从 mock success 变为 unavailable；这是 issue
  要求的有意行为变更，开发者需设置两个 env。
- Performance: validator 线性扫描已下载 bytes；无二次网络请求，额外 CPU 相对媒体下载很小。
- Maintenance: 结构 validator 不是完整编解码器；验收契约限定 PNG/MP4 基本有效性，未来
  扩展格式需显式增加 validator 与测试，不能静默接受。

## 测试计划

- [x] Unit tests: gateway registry 配置矩阵、mock catalog/fixtures；run PNG/MP4 validator。
- [x] Integration tests: invalid media provider 导致 step/run failed 且无 artifact/success event。
- [x] Regression: 现有显式 mock run、server workspace/provider catalog 测试。
- [x] Build/test: `cargo check --workspace`、`cargo test --workspace`。
- [x] Workflow: `python3 checks/check_workflow.py --repo . --spec-dir specs/GH115` 与 `--all-specs`。
- [x] Manual verification: PR 日志记录无 provider 配置的 catalog 快照和 focused test 结果；
  不发起真实付费调用。

## 回滚方案

回滚本 PR 恢复旧 registry/mock/persistence 行为；不涉及数据库 migration。若只需临时恢复
本地测试，应显式启用 mock 开关，而不是删除 fail-closed validator。生产回滚前必须确认
不会重新把占位媒体呈现为真实成功。
