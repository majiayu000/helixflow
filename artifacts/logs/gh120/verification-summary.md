# GH-120 Verification Summary

- Baseline: fresh `origin/main` at `8276fa2fe0760bead86d6812ab689ed266be9fab`.
- Manual repeated search: no existing GH120 spec, implementation, open PR, operation ID,
  or comments CAS path was found. Raw results are in `manual-search-*`.
- Red proof: `red-40-same-base-reproduction.log` records 40 same-base operations with
  39 accepted responses, but only 9 persisted comments and `seq=9`; one request failed.
- Spec gate: GH120 focused and all-specs checks passed. The user-authorized auto transition
  added `ready_to_implement`; live label evidence is trusted and implement route gate passed.
- Green proof: `green-40-same-base-retry.log` records first-wave `1` accepted and `39`
  conflicts, followed by refresh/retry convergence to 40 persisted comments and `seq=40`.
- Focused Store: 2 passed, including transaction/CAS/idempotency and 40-writer serialization.
- Focused migration: 1 passed.
- Focused server comments: 8 passed, including 409/currentSeq, idempotent replay,
  operation ID reuse, legacy import/fail-closed, presence, and add/resolve/delete.
- `cargo fmt --all -- --check`: passed.
- `cargo check --workspace`: passed with no warning.
- `cargo test --workspace`: 217 passed across workspace crates, 0 failed.
- `python3 checks/check_workflow.py --repo .`: passed.
- `python3 checks/check_workflow.py --repo . --spec-dir specs/GH120`: passed.
- `python3 checks/check_workflow.py --repo . --all-specs`: passed.
- `git diff --check`: passed.
