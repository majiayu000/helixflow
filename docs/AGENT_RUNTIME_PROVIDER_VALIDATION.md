# Agent Runtime Provider Validation

Status: companion validation and rollout plan for
[AGENT_RUNTIME_PROVIDER_SPEC.md](AGENT_RUNTIME_PROVIDER_SPEC.md).

## 1. Tests

### 1.1 Prompt Composer Tests

- chat prompt forbids ctx reads and shell commands;
- create prompt includes graph and catalogs;
- modify prompt requires smallest diff;
- runtime provider prompt does not hardcode Atlas;
- ComfyUI appears only as workflow backend/adapter;
- prompt section order is stable;
- echo guard is present;
- telemetry redacts local paths and secrets.

### 1.2 Router Tests

- `你好` -> `Chat`;
- `你是谁` -> `Chat`;
- `帮我做一个图生图工作流` -> `CreateWorkflow`;
- empty graph plus workflow request -> `CreateWorkflow`;
- non-empty graph plus `加一个 ControlNet` -> `ModifyWorkflow`;
- `运行这个 workflow` -> `RunRequest`;
- provider selection request -> `ClarificationForm` if ambiguous.

### 1.3 Provider Tests

- runtime catalog exposes arbitrary provider ids;
- Atlas connector works without special prompt code;
- disabled connector is not exposed to agent;
- missing credentials prevent provider execution;
- cost gate blocks over-threshold paid runs until user approval;
- backend rejects invented provider ids.

### 1.4 UI Tests

- plain chat renders without tool group when no visible tools ran;
- applied graph transaction refreshes the canvas and leaves rollback history;
- lifecycle JSON is hidden by default;
- prompt debug opens redacted section list;
- selected node comment scopes modification to target node/subgraph.

## 2. Rollout Plan

### Phase 1: Prompt Stack

- add `crates/agent/src/prompt.rs`;
- introduce `TurnMode`;
- render prompt sections;
- store prompt telemetry;
- keep existing output schemas.

### Phase 2: Catalog Split

- add workflow backend catalog;
- add runtime provider catalog;
- add API connector catalog;
- update node registry binding to connector capabilities.

### Phase 3: Capability-Gated Runs

- add capability grant model;
- route provider execution through backend;
- require confirmation only when estimated cost exceeds the configured threshold;
- log provider run evidence.

### Phase 4: Workflow Atoms

- add atom files under `ctx/atoms`;
- inject atoms by route/mode;
- add tests for atom selection.

### Phase 5: Prompt Debug UI

- expose redacted prompt telemetry endpoint;
- add "查看 prompt" debug affordance;
- keep normal chat clean.

## 3. Acceptance Criteria

1. Empty workspace has no graph until chat creates one.
2. Plain chat still calls the agent but does not read ctx or run shell.
3. Workflow creation produces a validated applied graph transaction.
4. Workflow modification preserves existing graph unless change is required.
5. ComfyUI is represented as workflow backend/adapter.
6. Atlas is represented as runtime provider and/or API connector data.
7. Arbitrary future APIs can be registered without prompt changes.
8. User-approved external provider execution never uses unlabeled mock output.
9. Provider execution never exposes credentials to the agent.
10. Prompt sections are stored with redacted telemetry.
11. UI shows chat first, applied changes in history, rollback controls, and logs as nested evidence.
