# Helixflow cross-machine continuation handoff

Date: 2026-08-13

Repository: `majiayu000/helixflow`

Primary continuation branch: `agent/ui-reliability-hardening-20260813`

## Read this first

This handoff preserves the recovered local workbench stack and the current UI
reliability tranche. Both pull requests are drafts. The code is suitable for
continuation and review, but it is not being presented as release-ready.

The branch stack is intentional:

```text
origin/main
  └─ agent/workbench-stack-20260813       # 26 recovered product commits + CI fix, draft PR #187
       └─ agent/ui-reliability-hardening-20260813
            # presence stabilization + current UI reliability work, stacked draft PR
```

Do not rebase the child branch directly onto `main` until PR #187 is merged or
the stack is deliberately restacked. Review and merge the base PR first.

The original pre-rebase recovery anchor remains available at
`origin/agent/local-workbench-preserve-20260813` (`7ab8258`). Do not delete it
until both draft PRs have landed and a release/recovery check has completed.

## Start on the other computer

```sh
git clone https://github.com/majiayu000/helixflow.git
cd helixflow
git fetch origin --prune
git switch --track origin/agent/ui-reliability-hardening-20260813
git status --short
git log --oneline --decorate -12
```

Install and verify with the repository-supported commands:

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked

cd web
npm ci
npm test
npm run build
```

The base draft can be inspected independently with:

```sh
git switch --track origin/agent/workbench-stack-20260813
```

## What was recovered and published

The base draft contains 26 commits that previously existed only in the primary
workstation checkout. They cover:

- durable conversations and terminal Agent turn records;
- Agent interruption, Codex thread/turn identity, and app-server resume;
- canvas state, proposal, and run tools;
- Atlas wired-image video input and Seedance image-to-video catalog support;
- orphaned-turn restart recovery, render recovery, and conversation-scoped
  progress;
- Atlas artifact materialization;
- artifact dock, canvas-first workbench, result selection, agent-first empty
  state, and edit-session cleanup.

The final recovered base commit is `653c15c` on
`agent/workbench-stack-20260813`. Draft PR #187 tracks this scope and closes
issue #185 when merged. A follow-up CI-only commit, `b677266`, refactors an
over-wide helper signature and satisfies the Rust 1.95 strict Clippy rules; it
does not add product scope.

The next commit, `29746a2`, stabilizes canvas presence updates and is kept in
the child reliability draft.

## Current UI reliability tranche

The child branch contains the following implemented work:

1. **Explicit central-canvas intent**
   - adds a typed optional `turnMode` field to the Web/API/Server contract;
   - records `TurnModeSource::Explicit` in durable message metadata;
   - makes the central “开始创建” surface send `create_workflow` even when the
     natural-language prompt contains no workflow keyword;
   - includes Agent-layer and Server-layer routing regression tests.

2. **Inspector pointer containment**
   - stops `pointerdown` and `click` propagation on the parameter disclosure;
   - preserves the selected node while opening parameters with a mouse;
   - includes a component-level regression test.

3. **Non-overlapping node placement**
   - searches bounded grid rings around the viewport center;
   - uses actual existing-node bounds plus a 24 px gap;
   - preserves explicit drag/drop coordinates;
   - includes deterministic helper coverage.

This tranche is tracked by issue #186. Both retained P0 items were completed
and browser-regressed on 2026-08-15.

## Completed P0 work

### P0-A: cancellation-safe durable Agent terminalization

Reproduction evidence showed an Agent output file could exist while the HTTP
request disappeared, leaving the durable turn permanently `running` and the
UI silent.

The implementation now provides an atomic Store settlement that:

- updates a turn only when it is still `running`;
- writes one user-visible terminal error/interrupted message in the same
  transaction;
- is safe when the normal success path already won the race;
- terminalizes graph-contract observations as well as `agent_turns`;
- is triggered by a request guard when the Axum handler future is dropped;
- is covered by a test that aborts a hanging request after persistence and
  waits for the durable terminal state.

The Web client also rejects a successful response with no terminal state or
messages. Browser regression exposed and fixed the missing
`agent_interrupted` Web message kind; interrupted turns now render the durable
“Agent 已停止” message instead of a schema error.

Relevant files:

- `crates/server/src/workbench_message.rs`
- `crates/server/src/agent_turn_control.rs`
- `crates/store/src/conversation_records.rs`
- `crates/server/src/workbench_message_tests.rs`
- `web/src/store.ts`
- `web/src/store-background.test.ts`

### P0-B: one provider/catalog/readiness truth

The UI audit reproduced this contradiction under the explicit local Mock
provider:

- the Inspector reported `BINDING_UNAVAILABLE`;
- the same graph could run successfully through Mock;
- the model tray still emphasized Atlas/FAL catalog entries.

Provider health, workspace-scoped catalog resolution, Intent compilation,
Inspector readiness, model-tray availability, and run preflight now consume
the same server-derived provider/capability readiness. Production connectors
remain fail-closed, and the explicit Mock double opt-in remains intact.

The 2026-08-15 regression used a fresh isolated data directory with explicit
Mock. It proved that Atlas can be healthy in the same process without its
catalog entries appearing executable in a Mock workspace, that an interrupted
request persists and displays a terminal message, and that fresh product-origin
console errors are empty.

Likely code map:

- `crates/registry/src/catalog_seed.rs`
- `crates/registry/src/resolver.rs`
- `crates/server/src/catalog_routes.rs`
- `crates/server/src/workbench_message_intent.rs`
- `crates/run/src/resolved.rs`
- `web/src/components/graph-canvas.tsx`
- `web/src/components/graph-canvas-inspector.tsx`
- `web/src/components/model-catalog-tray.tsx`

Integration coverage also proves that a FAL-selected workspace resolves the
FAL binding even when Atlas is the catalog default, while direct Mock
capabilities agree with run preflight.

## High-value P1 queue

After both P0 items pass browser regression:

- name a conversation from its first user message;
- replace raw binding/internal-field errors with structured recovery actions;
- show user-semantic long-task stages, last activity, interrupt, and retry;
- expand the output panel and select the newest artifact after first success;
- fit graph content after narrow/mobile state changes;
- move low-contrast technical metadata into a developer details surface.

Do not mix all P1 items into the current PR. Create bounded issues/PRs after
the P0 state contracts are stable.

## Audit evidence included in this branch

- `docs/handoffs/helixflow-ui-ux-audit-2026-08-13.md`
- `docs/handoffs/assets/helixflow-ui-ux-audit-2026-08-13/`

The directory contains the original 31 browser screenshots across desktop,
900 px, and 390 px views. The 2026-08-15 post-fix session repeated and captured
the same three viewport classes plus the 390 px expanded Chat state without
overwriting that baseline.

Completed browser retest:

1. fresh isolated data directory and explicit Mock provider;
2. ordinary creative prompt through the central canvas entry;
3. parameter disclosure by mouse, touch/pointer, and keyboard;
4. repeated node-library additions and drag/drop placement;
5. new conversation followed by a deliberately interrupted request;
6. provider/catalog/Inspector/run agreement;
7. desktop, 900 px, and 390 px screenshots;
8. browser console check for product-origin errors (fresh tab: none).

## Verification at handoff time

Passing from the final merged child worktree:

```text
cargo fmt --all -- --check
cargo check --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
(cd web && npm test)
(cd web && npm run build)
git diff --check
```

Results at handoff time were 553 Rust unit tests and 32 Web test files / 244
Web tests, all passing. The production Web build also passed. Re-run the full
suite after cloning to prove the destination toolchain and checkout.

## Canonical QA tracker

The only canonical tracker remains:

```text
docs/user_story_qa_20260630.csv
```

It was not updated during this tranche because the required
`@oai/artifact-tool` module was unavailable in the current environment. Do not
create a second competing tracker. On a machine with the required tool,
update the existing CSV from the audit and post-fix retest evidence.

## GitHub state and issue boundaries

At handoff creation:

- #143 is open because its only incomplete child is #146;
- #146 is correctly `needs_info`; do not delete the legacy contract early;
- #185 tracks publication of the recovered base stack;
- #186 tracks the current UI reliability tranche;
- PR #187 is the recovered base draft;
- the child draft targets `agent/workbench-stack-20260813`, not `main`.

Repository policy is one PR per issue. Keep future fixes scoped accordingly.

## GH146 canary is not stored in Git

The canary is an external operational asset and must not be committed. At the
time of this handoff it was healthy at `127.0.0.1:8787`, using exact release:

```text
release:       v0.2.0
build:         65504bf53f2cabff408518748fd471a709b1c369
deployment:    5880579119 (local-canary-recovery)
window start:  2026-08-13T02:04:30Z
earliest time: 2026-08-20T02:04:30Z
```

The canary still needs at least 20 human product graph-edit terminal
observations over three UTC days, the approved SLO rates, zero unattributed or
in-flight turns, and a rollback/restored-Intent drill on the same attributed
database. Synthetic traffic, fixtures, and smoke requests do not count.

A point-in-time SQLite online backup was created outside the repository at:

```text
/Users/lifcc/Desktop/code/AI/tools/graph/reports/transfer/
  helixflow-handoff-20260813/gh146-v020-canary.sqlite
```

SHA-256 at creation:

```text
2572c498aceb8fd894cd1205decd94c9fb42b77d9567589bc4b69de935f9953b
```

The backup passed `PRAGMA integrity_check` and contained zero formal-window
observations. It is a safety snapshot, not a substitute for a final cutover
backup. Before moving hosts, stop product use, take one final SQLite online
backup, verify its SHA-256 and integrity, then copy it separately with AirDrop,
`scp`, or another private channel. Never upload the database to GitHub.

The same private transfer directory also contains an optional offline Git
bundle with the base, child, and pre-rebase recovery branches:

```text
helixflow-continuation.bundle
```

The bundle's final SHA-256 is recorded in the adjacent private `TRANSFER.md`.
It is intentionally not embedded here because this file is itself contained
inside the bundle.

GitHub is the normal continuation path. Use the bundle only when the other
computer cannot reach GitHub or as an additional recovery copy.

Moving the canary to a different machine changes the deployment boundary.
Record the new host/deployment and either explicitly approve continuity or
start a new strict window; do not silently claim the old window continued.

## Other local worktrees

Most historical dirty worktrees contain only generated `.stack`, `.specrail`,
`artifacts`, logs, or `checks/__pycache__`. They were not placed into these PRs.
The notable exception is
`helixflow.wt/implx-gh101-self-heal-review`, whose three tracked diffs are only
Rust formatting changes in:

- `crates/run/src/tests.rs`
- `crates/server/src/artifact_routes.rs`
- `crates/server/src/sweep_support.rs`

Do not publish those stale formatting-only changes as a product PR. Review and
discard them only after the old worktree is no longer needed. Generated
evidence may be archived separately; it is not source code.

No historical worktree was deleted during this handoff.

## Safe completion order

1. Check out the child draft on the new computer.
2. Run the full clean verification commands.
3. Implement cancellation-safe durable terminalization and add abort tests.
4. Implement server-derived readiness convergence and integration tests.
5. Run the complete browser matrix and add a new screenshot set.
6. Update the canonical QA tracker when the required tool is available.
7. Review the base PR first, then the stacked reliability PR.
8. Continue GH146 evidence collection independently; do not merge legacy
   deletion into either workbench PR.
