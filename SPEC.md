# Specification Index

This repository's product and architecture specifications live in the existing
project documents. SpecRail is the workflow contract used to route and verify
agent-assisted changes.

## Helixflow Specs

- `SPEC_WORKFLOW_ORCHESTRATOR.md`: active workflow orchestrator direction.
- `specs/canvas-agent-full/`: current canvas-agent product, technical, task,
  and issue packet drafted from the canvas-agent research.
- `docs/CANVAS_BACKEND_FORMAT.zh.md`: backend canvas format proposal.

## SpecRail Contract

- `workflow.yaml`: route policy, artifacts, human gates, and automation policy.
- `states.yaml`: issue/spec/PR state machine.
- `labels.yaml`: label taxonomy expected by route gates.
- `templates/`: English and Chinese templates for issues, specs, tasks, PRs,
  and runtime checkpoints.
- `checks/`: deterministic validators and offline gates.
- `skills/`: repo-distributed Codex-compatible SpecRail skills pinned by
  `skills-lock.json`.

New GitHub-linked SpecRail packets should use `specs/GH<number>/` so
`python3 checks/check_workflow.py --repo . --all-specs` can validate them.
