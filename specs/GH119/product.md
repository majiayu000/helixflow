# Product Spec

## Linked Issue

GH-119：<https://github.com/majiayu000/helixflow/issues/119>

## 用户问题

Helixflow 当前把 provider 可影响的 `node_id` 拼入 artifact 文件名，并允许远端 artifact
下载自动跟随重定向、一次性读完整响应。恶意 workflow、被攻陷的 provider 或异常远端服务
因此可能把文件写出 artifact root、访问内网/本机地址，或用无界响应耗尽内存与磁盘；失败
消息还可能携带完整敏感 URL 或本地路径。

## 目标

- 所有持久化 artifact 使用服务端生成的 opaque filename，任何 run/step/node/path 输入都不能
  改变 artifact root 边界。
- 初始远端 URL 与每个 redirect target 都必须通过 HTTPS、host/IP 与地址范围策略，并把连接
  固定到已验证的 DNS 结果。
- 下载必须具有连接超时、总超时、redirect 上限、声明大小和实际流式字节上限；失败时清理
  部分文件。
- 只有响应 MIME、大小和 PR #117 媒体内容校验全部通过后，artifact 才能原子发布并进入成功
  状态。
- 安全拒绝必须 fail closed，错误只提供安全类别，不暴露凭据、完整远端 URL 或本地绝对路径。

## 非目标

- 不修改 provider API、认证、计费、远端任务取消或前端 artifact 展示。
- 不开放 HTTP、本地文件 URL、私网下载例外或 warning + fallback 降级路径。
- 不替换 PR #117 的 PNG/JPEG/MP4 内容 validator，也不实现通用媒体解码器。
- 不修改 `crates/server/**`、`crates/store/**`、`web/**` 或数据库 schema。

## Behavior Invariants

1. **P1 — 路径输入不可越界。** 包含 `..`、绝对路径、Unix/Windows 分隔符、平台特殊 component 或重复分隔符的 run/step/node 输入，均不能影响最终 artifact 文件名、目录结构或 root 外 canary；返回的 storage URI 必须是 root 内的安全相对路径。
2. **P2 — 服务端 opaque 命名。** 新 artifact filename 必须由服务端生成，且只含单一安全 component 与受限扩展名；客户端/graph/provider 标识不能出现在文件名中。并发写入不能覆盖已存在 artifact。
3. **P3 — canonical 与 lexical root 一致。** 写入前必须同时拒绝绝对/父目录 component，并验证已创建父目录的 canonical path 位于 canonical artifact root；指向 root 外的 symlink 不能承载 artifact。
4. **P4 — 每跳远端策略。** 初始 URL 和每个 redirect target 都必须是无凭据的 HTTPS URL；每一跳重新解析并拒绝 loopback、private、link-local、multicast、documentation、benchmark、reserved/unspecified 地址，不能只验证第一跳。
5. **P5 — DNS 结果固定。** hostname 的所有候选地址都必须通过 P4，当前连接只能使用本跳已验证的解析结果；混合 public/private DNS 结果必须整体拒绝，避免 DNS rebinding 或客户端二次解析绕过。
6. **P6 — 重定向有界。** 客户端不得自动重定向；redirect 缺少/非法 `Location`、循环或超过最大跳数时必须失败，且每个新 target 重新执行 P4/P5。
7. **P7 — 时间与大小有界。** 下载必须同时受连接超时、整个 redirect+body 总超时、`Content-Length` 上限和实际流式接收字节上限约束；缺少或伪造 `Content-Length` 不能绕过实际上限。
8. **P8 — MIME 与内容成功门禁。** 成功响应的 `Content-Type` 必须与 payload 声明兼容；下载完成后继续执行 PR #117 的 kind/MIME/media validator。HTTP 成功但 MIME 或媒体内容无效时不得发布文件或成功 artifact。
9. **P9 — 失败清理与状态可信。** 网络、安全、大小、MIME、内容、写入或 rename 任一失败都必须清理 `.part` 文件，不创建最终 artifact；现有 executor 必须收到 error，不能把 step/run 标记为 `succeeded`。
10. **P10 — 错误信息最小披露。** Artifact persistence 错误不得包含 URL userinfo、query、fragment、完整敏感 URL、响应 body 或本地绝对路径；安全失败不得降级为 warning 或继续使用 `storage_uri`。
11. **P11 — 合法路径兼容。** 合法 inline text/JSON/PNG/JPEG/MP4 继续写入 root 内；合法 public HTTPS 响应在 MIME、大小与媒体 validator 通过时继续成功。

## 验收标准

- [x] traversal、绝对路径、Unix/Windows 分隔符与 root 外 symlink 测试证明最终文件始终留在 root，root 外 canary 不变。
- [x] filename 为服务端 opaque 单 component；并发/碰撞场景不覆盖既有文件，写入采用同目录临时文件后原子发布。
- [x] 初始 URL 与每个 redirect target 均执行 HTTPS、host/IP、DNS pin 与 forbidden range 策略；loopback/private/link-local/reserved 目标全部被拒绝。
- [x] 连接/总超时、redirect 数、`Content-Length` 和实际流式字节上限都有确定性测试；失败后无最终文件与 `.part` 残留。
- [x] 合法 HTTPS/MIME/媒体路径继续通过 PR #117 validator，非法 MIME/媒体不能成功。
- [x] focused tests、`cargo check --workspace`、`cargo test --workspace`、GH119 spec 与 all-specs checks 全部通过。

## 边界情况

- URL 使用 IP literal、IPv4-mapped IPv6、大小写 hostname、显式端口、相对 redirect 或缺少
  `Location` 时仍执行同一策略。
- DNS 返回空集合、解析失败、混合公网/非公网地址或重定向回已访问 URL时 fail closed。
- `Content-Length` 缺失时允许继续有界流式读取；大于上限时在读取 body 前拒绝。
- 临时文件创建后发生 timeout、chunk 错误、内容无效或最终发布失败时必须尝试清理；清理失败
  不能掩盖原始安全错误。

## 发布说明

这是 artifact I/O 的安全收紧。此前可下载的私网、保留地址、HTTP、MIME 不匹配或超限响应
会改为明确失败；合法 public HTTPS provider artifact 与现有 inline artifact 继续兼容。该变更
不包含数据库迁移或前端变更。
