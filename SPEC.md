# Helixflow Specification Index

Helixflow is a local-first AI workflow orchestrator. Its product contract is
implemented by the Rust workspace and React workbench and documented in the
following sources:

- `SPEC_WORKFLOW_ORCHESTRATOR.md`: primary workflow-orchestrator direction.
- `docs/AGENT_RUNTIME_PROVIDER_SPEC.md`: agent/runtime-provider boundaries.
- `docs/CANVAS_BACKEND_FORMAT.zh.md`: backend canvas format proposal.
- `docs/`: product research, roadmaps, validation notes, and design material.
- `specs/`: historical issue-linked product, technical, and task records.

## Historical Workflow Records

Files under `specs/` are retained for product history and implementation
traceability. They may mention superseded repository workflows, checks, or
gates; those references are historical context, not executable instructions.

The former repo-local SpecRail execution pack and its CI gate are retired.
Nothing in this repository should automatically load, invoke, or install that
workflow. The original adoption snapshot remains available in Git commit
`af577c00993499b45ab6ae962e89b14be729ebd6`.
