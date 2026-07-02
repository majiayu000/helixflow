# AGENTS.md

This repository uses SpecRail as its repo-local workflow contract for agent
work. Keep implementation changes small, explicit, and backed by fresh
verification.

## Agent Entry

- Read `AGENT_USAGE.md` before creating issues, specs, PR bodies, reviews, or
  handoffs.
- Treat `workflow.yaml`, `states.yaml`, `labels.yaml`, and `templates/` as the
  durable SpecRail contract.
- Load `skills/specrail-workflow/SKILL.md` first for SpecRail routing, then load
  exactly one focused SpecRail skill for the selected route.
- If the user writes Chinese, write human-facing specs, issues, PR text,
  handoffs, and explanations in Chinese. Keep stable IDs, commands, paths, and
  JSON keys in English.

## Repository Rules

- Search existing files and specs before adding new ones.
- Product-facing, architecture, cross-module, public API, workflow-policy, and
  ambiguous work must go through SpecRail mode unless the user explicitly asks
  for a narrow direct edit.
- Preserve human gates: agents must not approve, merge, force-push, change
  permissions, publish security disclosures, or close disputed work without
  explicit human authorization.
- Keep dirty worktree boundaries intact. Do not revert or stage unrelated user
  changes.
- Existing unnumbered planning artifacts may stay in place, but new
  GitHub-linked SpecRail packets should use `specs/GH<number>/`.

## Validation

Run these after changing SpecRail workflow assets:

```sh
python3 checks/check_workflow.py --repo .
python3 tools/install_codex_skills.py --repo .
```

For Rust, TypeScript, Go, or Python code changes, also run the relevant project
build and test commands for the touched code path.
