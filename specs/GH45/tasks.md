# Task Plan: GraphCanvas Selection, Shortcuts, And Clipboard Ergonomics

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/45
Locale: zh-CN

## Scope

Implement one focused frontend PR for GraphCanvas selection and clipboard ergonomics. Keep destructive graph edits, paste-to-mutate, node deletion, manual proposal editing, backend changes, and agent canvas ops out of this issue.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP45-T1 | frontend | none | Add pure selection, shortcut, fit-view, and clipboard helper functions. | Helper tests cover rectangle selection, shortcut guards, clipboard sanitization, and stable output. | `cd web && npm test -- app.test.tsx` |
| SP45-T2 | frontend | SP45-T1 | Add GraphCanvas box selection and selection overlay. | Edit-mode drag or modifier drag selects nodes inside the area without breaking pan in view mode. | `cd web && npm test -- app.test.tsx` |
| SP45-T3 | frontend | SP45-T1 | Add canvas keyboard shortcuts. | Escape, Cmd/Ctrl+A, Cmd/Ctrl+0, Cmd/Ctrl+C work when focus is not in editable input and skip IME composition. | `cd web && npm test -- app.test.tsx` |
| SP45-T4 | frontend | SP45-T1-SP45-T3 | Add multi-selection inspector summary and clipboard status. | Multi-select shows a summary; copy writes sanitized selection text. | `cd web && npm test -- app.test.tsx` |
| SP45-T5 | verification | SP45-T1-SP45-T4 | Run deterministic verification and inspect diff. | Fresh checks pass and no backend/API behavior is changed. | `cd web && npm run build`; `cargo check --workspace`; `cargo test --workspace`; `git diff --check` |

## Thread Ownership

- Frontend implementation lane owns `web/src/components/graph-canvas.tsx`, `graph-canvas-selection.ts`, `graph-canvas-inspector.tsx`, `graph-canvas-navigation.ts`, `web/src/canvas.css`, and `web/src/app.test.tsx`.
- Read-only review lane owns shortcut/IME/clipboard risk review and must not edit files.
- Coordinator owns SpecRail artifacts, final verification, PR gate, merge, and next-issue handoff.

## Handoff Notes

- Do not add deletion, paste mutation, manual graph editing, backend routes, or schema changes.
- Keep shortcuts out of textarea/input/select/contenteditable and IME composition.
- Clipboard output must omit provider fields and redact local paths/token-like strings.
- User has explicitly authorized commit, PR creation, and merge after SpecRail gate and verification.
