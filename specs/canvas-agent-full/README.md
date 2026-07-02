# Canvas Agent Full Feature Spec Packet

This packet defines the remaining work needed to turn the current canvas backend foundation into a complete canvas-agent product experience.

Scope status on 2026-07-02:

- Remote GitHub open issues: none found.
- Remote GitHub open PRs: none found.
- Repo-local SpecRail markers: not present.
- Existing implementation: backend `CanvasDocument` / `CanvasOpEnvelope` / store / REST / WS foundation exists, but the React canvas UI still primarily uses `WorkbenchState.graph`.

Files:

- `product.md`: user-facing product specification.
- `tech.md`: engineering design and affected areas.
- `tasks.md`: implementation task plan.
- `issues.md`: GH-ready issue drafts.

Issue map:

- `CANVAS-001`: Make React canvas consume `CanvasDocument` as source of truth.
- `CANVAS-002`: Convert canvas editing actions into durable `canvas_ops`.
- `CANVAS-003`: Add comments and presence collaboration UX.
- `CANVAS-004`: Complete run/job/artifact backfill into visible canvas nodes.
- `CANVAS-005`: Add canvas session ticket, reconnect, and sync hardening.
- `CANVAS-006`: Add migration, compatibility, and end-to-end regression coverage.

Human gate:

- These are local issue drafts. Do not create remote GitHub issues from this packet unless the user explicitly asks to publish them.
