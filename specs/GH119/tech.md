# Tech Spec

## Linked Issue

GH-119：<https://github.com/majiayu000/helixflow/issues/119>

## Product Spec

[`product.md`](product.md)

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Artifact orchestration/media validation | `crates/run/src/artifacts.rs` | inline/remote 共用 PR #117 media validator，但 remote 使用默认 redirect 与全量 `bytes()`；文件名拼接 `node_id` | P1/P4/P7/P8 的直接根因和必须保留的成功门禁 |
| Artifact path boundary | 新增 `crates/run/src/artifact_path.rs` | `root.join(relative)` 后直接 `create_dir_all`/`write`，没有 lexical/canonical boundary 或原子发布 | P1/P2/P3/P9 |
| Restricted remote fetch | 新增 `crates/run/src/artifact_remote.rs` | 没有 URL/地址范围/DNS pin/redirect/timeout/size/MIME policy | P4–P10 |
| Executor persistence | `crates/run/src/executor.rs`（只读） | persistence error 已向上传播，阻止 DB artifact 与 step/run success | 新模块应复用错误流，无需跨 ownership 修改 |
| Gateway payload contract | `crates/gateway/src/lib.rs`（只读） | `RemoteUrl { url }`、payload `kind`/`mime` 提供策略输入 | 保持公共 API，不把 secret 或内部地址加入新字段 |

## 设计方案

### 1. 服务端 opaque path 与 root boundary

- `artifact_relative_path()` 不再使用 `run_id`、`step_id` 或 `node_id`；生成
  `artifacts/<UUIDv7>.<safe_extension>`。调用签名保留这些参数只为兼容内部 call site 时，参数
  改为未使用；返回值只包含 normal components。
- `artifact_path.rs` 提供单一写入入口：先检查 relative 非空、非 absolute、只含
  `Component::Normal`，再创建 canonical root 和 `artifacts` parent。
- canonicalize parent 后要求其 `starts_with(canonical_root)`；若 `root/artifacts` 是指向 root 外
  的 symlink，立即返回安全、无路径回显的 `ArtifactPersistence` error。
- 在 canonical parent 内用 `create_new(true)` 创建 opaque `.part` 文件，完成 write/flush/sync 后
  rename 到另一个 opaque final name。最终文件名不可由 provider 控制；UUID 碰撞时重新生成，
  不覆盖既有 final artifact。
- 所有失败分支 best-effort 删除 `.part`；清理错误不替换原始错误。返回 logical relative path，
  不返回 canonical/绝对路径。

### 2. 受限 URL、地址范围与 DNS pin

- `artifact_remote.rs` 定义私有常量策略：connect timeout、total timeout、最大 redirect 和最大
  artifact bytes。错误只用稳定类别文案，不格式化 reqwest error 或 URL。
- 每跳 `validate_url()` 要求 `https`、可用 host、无 username/password；IP literal 直接进入地址
  策略，hostname 先拒绝 local-only suffix，再用 `tokio::net::lookup_host` 解析。
- IPv4 拒绝 unspecified/loopback/private/link-local/shared/documentation/benchmark/multicast/
  reserved；IPv6 拒绝 unspecified/loopback/IPv4-mapped forbidden/ULA/link-local/site-local/
  documentation/multicast/非 global-unicast 前缀。
- hostname 的任一结果不安全即整体拒绝。每跳创建 `redirect::Policy::none()` client，并通过
  `resolve_to_addrs` 把 hostname 固定到已验证地址；因此 reqwest 不能在检查后使用未验证 DNS
  结果。

### 3. 手动 redirect 与总时间预算

- 使用外层 `tokio::time::timeout(TOTAL_TIMEOUT, download_loop(...))` 覆盖 DNS、所有 redirect、
  headers、body 和落盘。
- 每次仅发送一跳，3xx 时读取并 join `Location`，检查 visited URL 与 redirect count，丢弃旧
  response 后从 URL、DNS 到连接重新验证。非 success/redirect 均返回仅含 HTTP status code 的
 安全错误。
- 默认 client 不携带 provider secret/header/cookie；redirect 不转发任何凭据。URL userinfo 在
  发请求前被拒绝。

### 4. 有界流式写入、MIME 与 media validation

- success response 先验证 `Content-Type`：解析后与 payload 声明 MIME 的 type/subtype 相同；
  缺失、非法或不匹配均 fail closed。`Content-Length` 若存在且大于上限，在创建临时文件前拒绝。
- body 通过 `Response::chunk()` 流式读取；每次用 checked addition 更新实际字节数，超过上限
  立即停止并清理 `.part`。不调用无界 `bytes()`。
- 下载结束后从有界临时文件读取 bytes，调用 `artifacts.rs` 现有
  `validate_artifact_bytes(payload, bytes)`，成功后才执行原子 rename。这样保留 PR #117 的
  PNG/JPEG/MP4 validator，且最终文件只代表完整成功。
- inline artifact 使用同一 path writer，但在创建临时文件前先执行现有 validator；Text/JSON
  行为不变。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1/P2/P3 | `artifact_path.rs` | traversal/absolute/separator 输入、opaque filename、root 外 symlink/canary、已存在文件不覆盖 |
| P4/P5 | `artifact_remote.rs` URL/IP/DNS helpers | loopback/private/link-local/shared/docs/benchmark/reserved、IPv4-mapped IPv6、混合 DNS tests |
| P6 | manual redirect state machine | 每跳策略、相对 redirect、loop/缺 Location/超过最大 redirect tests |
| P7 | timeout/length/chunk accounting | connect/total timeout 配置断言、oversized `Content-Length`、无 length 实际超限 tests |
| P8 | MIME gate + `validate_artifact_bytes` | mismatch/missing MIME 拒绝；合法 PNG/MP4 回归 |
| P9 | temp-file guard/path writer | chunk/timeout/validation/publish error 后无 final/`.part` 文件 |
| P10 | stable error constructors | secret URL 输入与网络错误结果不含 userinfo/query/完整 URL/绝对 root |
| P11 | existing + new run tests | inline text/JSON/media 和合法 public HTTPS policy fixture 回归 |

## 数据流

1. executor 把 provider `ArtifactPayload` 交给 `persist_provider_artifact()`。
2. inline content 先执行现有 kind/MIME/media validator，再请求 path writer 生成 opaque relative
   path、边界校验、临时写入与原子发布。
3. remote content 先解析 URL，在总 timeout 内逐跳执行 URL → DNS/address → pinned client →
   response policy；3xx 回到逐跳入口。
4. success response 通过 MIME/声明大小后，流式写入 root 内 `.part` 并累计实际大小。
5. body 完成后读取有界 bytes，执行 PR #117 validator；成功才 rename 为 final artifact，并把
   safe relative path 返回 executor。
6. 任一错误删除 partial，executor 沿现有 `RunResult` 路径把 step/run 标记 failed，数据库不会
  创建 artifact。

## 备选方案

- **仅清洗 `node_id`：拒绝。** allowlist 容易产生平台差异/别名，且 run/step 仍无必要进入
  filename；opaque server id 更小、更可靠。
- **只做 lexical `starts_with`：拒绝。** symlink parent 可让 lexical root 内路径实际写出 root，
  需要 canonical parent 检查。
- **先下载到内存再检查大小：拒绝。** 无法防止内存耗尽；必须边接收边计数。
- **只检查初始 URL 后让 reqwest 自动 redirect：拒绝。** redirect 可转向私网；必须禁用自动
  redirect 并逐跳重验。
- **解析 DNS 后仍用默认 client：拒绝。** 存在检查/连接二次解析差异；必须 pin 已验证结果。
- **引入 server allowlist/config：暂不采用。** 本 issue 要求统一安全默认边界；新增公开配置会
  扩大 cross-module scope，且错误配置可能重新开放 SSRF。

## 风险

- Security: DNS pin、每跳重验、symlink parent 与 partial cleanup 都是 fail-closed 路径；错误
  文案不能直接包含 reqwest/io error，因为其中可能含 URL 或绝对路径。
- Compatibility: 私网/self-hosted artifact URL、缺失 Content-Type 或 MIME 不匹配响应会从旧成功
  变为失败；这是安全目标要求的有意收紧。
- Performance: 下载改为流式磁盘写入，峰值网络接收有界；PR #117 validator 仍需读取受限大小
  文件到内存，最大值提供明确上限。
- Maintenance: reserved range table 需以单元测试锁定；未来放宽必须新增明确产品 acceptance，
  不能通过 warning/fallback 绕开。

## 测试计划

- [x] Unit tests: safe relative path、canonical root/symlink、opaque naming、IP/range/URL/redirect/
  MIME/size/error-redaction helpers。
- [x] Async focused tests: partial write cleanup、实际 chunk 超限、redirect 每跳重验、合法 HTTPS/
  media fixture；不访问付费 provider。
- [x] Regression: 现有 run artifact/executor/media tests。
- [x] Build/test: `cargo check --workspace`、`cargo test --workspace`。
- [x] Workflow: GH119 spec、base workflow 与 all-specs checks。

## 回滚方案

该变更无 schema migration。代码回滚会重新开放已知越界写入/SSRF/资源耗尽风险，因此只应在
安全回归确认后整体回滚；不能仅关闭地址或大小策略。若合法 provider 因响应 MIME 或大小不兼容，
应修复 provider 契约或单独评审新的受限配置，而不是恢复默认无界下载。
