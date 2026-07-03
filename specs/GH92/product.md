# Product Spec

## Linked Issue

GH-92

## 用户问题

Canvas WebSocket 目前缺少专用 ticket gate 和稳健 reconnect/catch-up 协议。
连接中断、重复 retry 或 seq gap 可能导致用户看到 stale state 或重复 op。

## 目标

- 添加 `POST /api/canvases/{canvas_id}/ticket`。
- ticket mode 启用时，用 ticket gate canvas WebSocket。
- 前端记录 last accepted canvas `seq`。
- Reconnect 时先请求 `/events?afterSeq={seq}`。
- 检测 seq gap 并通过 event fetch 恢复。
- 保留 WS 离线时的 REST fallback。

## 非目标

- 不接入远程 identity provider。
- 不在代码中硬编码长期 secret。
- 不改变 durable op 格式。

## Behavior Invariants

1. ticket mode 启用时，缺失或无效 ticket 被拒绝。
2. local dev 可通过 env config 关闭 ticket enforcement。
3. Reconnect 会从 last accepted seq catch up。
4. Seq gap 必须先 fetch missing events，再应用新 WS event。
5. Retry/idempotency 不能产生重复 op。

## 验收标准

- [ ] Missing/invalid ticket is rejected when ticket mode is enabled.
- [ ] Reconnect from stale `seq` catches up without duplicate ops.
- [ ] Gap detection fetches missing events before applying new WS events.
- [ ] Local development can run with ticket enforcement disabled by env config.

## 边界情况

- ticket 过期必须返回明确错误。
- event fetch 失败时不能继续应用可能错序的 WS event。
- env config 解析失败必须 fail closed 或明确报错。

## 发布说明

该变更增加可选 ticket enforcement 与 reconnect hardening；默认本地开发兼容策略
由 env config 决定。
