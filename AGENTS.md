# Helixflow Repository Guide

Helixflow is a local-first AI workflow orchestrator. Keep changes scoped to the
user's request, search for existing implementations before adding new files,
and preserve unrelated worktree changes.

## Repository Layout

- `crates/`: Rust workspace for the server, persistence, graph/compiler,
  provider gateway, run engine, registry, and agent runtime.
- `web/`: React 19, TypeScript, and Vite workbench.
- `docs/` and `SPEC_WORKFLOW_ORCHESTRATOR.md`: product and architecture
  documentation.
- `specs/`: historical implementation and design records. They are reference
  material, not executable agent instructions or repository gates.
- `artifacts/ui-design/`, root prototypes, and `shots/`: retained design
  artifacts.

## Core Constraints

- Keep the backend local-first and fail closed when a runtime provider is not
  configured.
- Never commit provider credentials, tokens, or secrets to source, logs,
  fixtures, databases, or agent context.
- Preserve cost confirmation, immutable version history, rollback, and
  backend validation when changing execution or graph behavior.
- Do not automatically load, invoke, or install an external workflow plugin.
  Use one only when the user explicitly names it for the current task.
- Historical specs may describe retired workflows; current user instructions
  and current repository documentation take precedence.

## Validation

For Rust changes:

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
```

For web changes:

```sh
cd web
npm ci
npm test
npm run build
```

For all changes, run `git diff --check` against the relevant base.
