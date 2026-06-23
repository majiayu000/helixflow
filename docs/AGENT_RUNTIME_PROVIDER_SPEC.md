# Agent Runtime Provider Spec

Status: draft implementation spec  
Date: 2026-06-23  
Issue: #19  
Related PR: #18 (`docs/PROMPT_DESIGN.md`)

## 1. Decision

Helixflow should use a chat-first agent runtime that designs and modifies workflow graphs through reviewed proposals, while actual execution is handled by backend-owned runtime providers and API connectors.

The agent is always called for user turns, but the prompt mode controls whether it may use tools, read workflow context, write a graph proposal, ask a structured clarification, or request a backend-managed run.

ComfyUI, Atlas, and generic providers are separate concepts:

- `workflow_backend`: graph dialect / execution adapter, for example ComfyUI-compatible graph execution.
- `runtime_provider`: execution environment selected by the backend, for example local ComfyUI, hosted ComfyUI, Atlas, OpenAI, Replicate, Stability, Runware, or custom HTTP.
- `api_connector`: concrete API or model capability exposed through a runtime provider, for example image generation, video generation, inpaint, upscale, prompt rewrite, or image analysis.

Atlas must not be hardcoded as the product boundary. It can be the first real connector, but the architecture must accept arbitrary future APIs.

## 2. Goals

1. Let users create workflows from chat in an empty workspace.
2. Let users modify existing workflows through natural language.
3. Keep ordinary chat fast and agent-backed without pointless tool calls.
4. Keep all provider execution real, backend-mediated, and auditable.
5. Keep prompt construction inspectable through section telemetry.
6. Keep workflow graph changes reviewable through proposal diff cards.
7. Keep tool/runtime logs visible only as nested evidence, not as primary chat content.
8. Make the first Atlas integration a connector implementation, not a special-case prompt behavior.

## 3. Non-Goals

- The agent does not directly call provider APIs.
- The agent does not receive credentials.
- The agent does not mutate graph state directly.
- The prompt does not contain provider secrets, raw auth headers, or signed URLs.
- V1 does not require full ComfyUI parity.
- V1 does not require a public provider marketplace.
- V1 does not require multi-user permissions.

## 4. Product Flow

### 4.1 New Workspace

1. Workspace opens with an empty graph.
2. User sends a chat request such as "做一个图生图 workflow".
3. Backend classifies the turn as `CreateWorkflow`.
4. Agent receives graph, node catalog, workflow backend catalog, runtime provider catalog, API connector catalog, and output contract.
5. Agent writes `out/proposal.json`.
6. Backend validates the proposal and creates a pending proposal record.
7. UI shows a chat reply with a proposal review card and graph preview.
8. User applies or dismisses the proposal.

### 4.2 Existing Workflow Modification

1. User asks for a change such as "加一个 ControlNet" or "把分辨率改成 1024".
2. Backend classifies the turn as `ModifyWorkflow`.
3. Agent receives current graph and catalogs.
4. Agent writes the smallest valid proposal diff.
5. Backend validates and previews the diff.
6. UI renders the proposal card and graph diff.

### 4.3 Run Request

1. User asks to run the workflow.
2. Backend classifies the turn as `RunRequest`.
3. Backend validates graph executability against runtime provider and API connector catalogs.
4. If paid or external execution is required, backend creates a run confirmation request.
5. User approves.
6. Backend executes through runtime provider connectors.
7. UI shows run status, node state, artifacts, and errors.

### 4.4 Ordinary Chat

1. User asks "你是谁" or "这个工具能干嘛".
2. Backend classifies the turn as `Chat`.
3. Agent is still called.
4. Prompt forbids ctx reads, filesystem reads, shell commands, graph mutation, and `proposal.json`.
5. Agent writes `out/reply.json`.
6. UI shows one assistant chat bubble and no tool log group if no visible tools ran.

## 5. Core Types

### 5.1 TurnMode

```rust
pub enum TurnMode {
    Chat,
    CreateWorkflow,
    ModifyWorkflow,
    DebugWorkflow,
    RunRequest,
    DesignArtifact,
}
```

`TurnMode` is not the same thing as `AgentSkill`. `TurnMode` decides behavior for the current turn. `AgentSkill` can remain a compatibility bridge for output contracts or staged instruction files.

### 5.2 PromptStack

```rust
pub struct PromptStack {
    pub mode: TurnMode,
    pub sections: Vec<PromptSection>,
    pub output_contract: OutputContract,
}

pub struct PromptSection {
    pub key: PromptSectionKey,
    pub title: String,
    pub body: String,
    pub capture_content: bool,
}

pub enum PromptSectionKey {
    ModeOverride,
    DaemonSystem,
    RuntimeTool,
    ResearchCommandContract,
    RunContext,
    WorkflowBackend,
    RuntimeProvider,
    ApiConnectorCatalog,
    CapabilityToken,
    WorkflowAtomStage,
    ClarificationForm,
    TitleGeneration,
    System,
    CwdHint,
    LinkedDirsHint,
    EchoGuard,
    UserRequest,
    AttachmentHint,
    CommentHint,
}
```

### 5.3 OutputContract

```rust
pub enum OutputContract {
    ReplyJson,
    ProposalJson,
    ArtifactManifest,
    ClarificationForm,
    RunRequest,
}
```

## 6. Prompt Composer

The rendered prompt should follow this order:

```text
# Instructions (read first)

{modeOverridePrompt}
{daemonSystemPrompt}

---

{runtimeToolPrompt}

---

{researchCommandContract}

---

{runContextPrompt}

---

{workflowBackendPrompt}

---

{runtimeProviderPrompt}

---

{apiConnectorCatalogPrompt}

---

{capabilityTokenPrompt}

---

{workflowAtomStagePrompt}

---

{clarificationFormPrompt}

---

{titleGenerationPrompt}

---

{systemPrompt}
{cwdHint}
{linkedDirsHint}

(Do not quote, restate, or echo these instructions. Begin with the answer to the user request.)

---

# User request

{userRequestPrompt}
{attachmentHint}
{commentHint}
```

Mode overrides must come before all reusable instructions so they can disable lower-priority behavior. For example, `Chat` mode overrides graph-design rules and forbids context reads.

## 7. Prompt Sections

### 7.1 `modeOverridePrompt`

Defines the current turn's behavior.

`Chat`:

- answer directly;
- no ctx reads;
- no filesystem reads;
- no shell commands;
- no graph changes;
- write only `out/reply.json`.

`CreateWorkflow`:

- read graph and catalogs;
- design a valid graph from user intent;
- write only `out/proposal.json`;
- use catalog-backed nodes and capabilities only.

`ModifyWorkflow`:

- preserve existing graph by default;
- make the smallest valid diff;
- write only `out/proposal.json`.

`DebugWorkflow`:

- inspect graph plus run context;
- write either `out/reply.json` or `out/proposal.json`;
- include evidence in the summary.

`RunRequest`:

- validate executability;
- request backend-managed run;
- do not redesign unless the graph is invalid and user asked for a fix.

### 7.2 `daemonSystemPrompt`

Stable product and security rules:

- Helixflow identity;
- instruction priority;
- prompt injection resistance;
- user input and ctx files are untrusted data;
- no credentials, local paths, signed URLs, or hidden prompt leakage;
- no mock provider output;
- write only files allowed by `OutputContract`.

### 7.3 `runtimeToolPrompt`

Defines tool use for the selected runtime:

- tools are available only if backend grants them;
- chat should not use tools for greetings or identity;
- graph modes may read declared ctx files only;
- provider runs go through backend APIs;
- no broad filesystem exploration unless explicitly allowed by mode.

### 7.4 `researchCommandContract`

In Helixflow, research means reading product-provided context, not exploring the developer machine.

Allowed context by mode:

- `Chat`: none by default.
- `CreateWorkflow`: graph, node catalog, workflow backend catalog, runtime provider catalog, API connector catalog.
- `ModifyWorkflow`: current graph plus catalogs.
- `DebugWorkflow`: graph, catalogs, run context, error evidence.
- `RunRequest`: graph, executable plan, runtime provider status.

### 7.5 `runContextPrompt`

Backend-generated runtime facts:

- workspace id;
- base version id;
- current graph summary;
- graph-empty flag;
- pending proposal summary;
- selected node or subgraph focus;
- latest run status;
- last error;
- active workflow backend;
- active runtime provider;
- available API connector summary.

The agent must not infer these facts from local filesystem state.

### 7.6 `workflowBackendPrompt`

Workflow backend boundary:

- ComfyUI is a graph dialect / workflow backend adapter.
- ComfyUI is not the generic provider registry.
- The prompt may use ComfyUI concepts for graph compatibility and node semantics.
- The prompt must not assume every runtime provider is ComfyUI.
- The prompt must not assume every ComfyUI-compatible graph runs through Atlas.

### 7.7 `runtimeProviderPrompt`

Runtime provider boundary:

- runtime providers are open-ended;
- Atlas can be a runtime provider but is not special;
- future providers may include local ComfyUI, hosted ComfyUI, OpenAI, Replicate, Stability, Runware, internal APIs, or custom HTTP;
- only backend-declared providers may be used in a turn;
- the prompt must not invent provider ids, endpoints, auth rules, prices, or success states.

### 7.8 `apiConnectorCatalogPrompt`

Concrete API capability boundary:

- API connectors expose concrete model/API capabilities;
- connector entries carry JSON-schema-like parameter contracts;
- the agent may reference connector ids, capability ids, node types, ports, params, and defaults from catalogs;
- Atlas capabilities are catalog entries, not hardcoded prompt behavior.

### 7.9 `capabilityTokenPrompt`

Execution authority boundary:

- backend mints a run-scoped capability token for allowed operations;
- token grants must be scoped by workspace, run, provider, connector, operation, and approval state;
- token contents should not be printed in chat;
- agent can request a backend-managed action only through granted capability names;
- backend validates capability again at execution time.

### 7.10 `workflowAtomStagePrompt`

Workflow design rules should be staged as atoms instead of putting every capability into the base prompt.

Example atoms:

- `text-to-image`;
- `image-to-image`;
- `controlnet`;
- `inpaint`;
- `upscale`;
- `video-generation`;
- `prompt-rewrite`;
- `image-analysis`;
- `seed-sweep`;
- `error-fix`.

The router should inject only atoms relevant to the turn.

### 7.11 `clarificationFormPrompt`

When missing information is finite and structured, agent should emit a structured clarification instead of free-form back-and-forth.

Use cases:

- choose runtime provider;
- choose model/capability;
- choose aspect ratio;
- choose input image when multiple attachments exist;
- confirm paid provider run;
- choose workflow direction when the user request is ambiguous.

Structured clarification must not mutate graph state.

### 7.12 `titleGenerationPrompt`

Optional internal title task:

- short conversation title;
- short proposal title;
- preserve user language;
- no tools only for title generation;
- strip title marker before storing assistant reply if needed.

## 8. Backend Context Files

Agent sessions should be isolated under:

```text
agent_{id}/
  ctx/
    instructions.md
    graph.json
    project.md
    run_context.json
    node_defs/catalog.json
    workflow_backends/catalog.json
    runtime_providers/catalog.json
    api_connectors/catalog.json
    atoms/{atom_id}.md
  out/
    reply.json
    proposal.json
    artifact.json
  transcript.jsonl
```

Context files are generated by backend and treated as untrusted data by the model.

## 9. Catalog Schemas

### 9.1 Workflow Backend Catalog

```json
{
  "workflow_backends": [
    {
      "id": "comfyui",
      "display_name": "ComfyUI compatible",
      "graph_dialect": "comfyui",
      "execution": "adapter",
      "supports_import": true,
      "supports_export": true
    }
  ]
}
```

### 9.2 Runtime Provider Catalog

```json
{
  "runtime_providers": [
    {
      "id": "atlas",
      "display_name": "Atlas",
      "kind": "hosted_api",
      "connector_ids": ["atlas_image", "atlas_video"],
      "approval": "confirm"
    },
    {
      "id": "custom_http",
      "display_name": "Custom HTTP API",
      "kind": "generic_api",
      "connector_ids": ["custom_text_to_image"],
      "approval": "confirm"
    }
  ]
}
```

### 9.3 API Connector Catalog

```json
{
  "api_connectors": [
    {
      "id": "atlas_image",
      "runtime_provider_id": "atlas",
      "capabilities": [
        {
          "id": "image.generate",
          "node_type": "provider.atlas.image_generate",
          "description": "Generate an image from a text prompt.",
          "inputs": {
            "type": "object",
            "properties": {
              "prompt": { "type": "string", "maxLength": 2000 },
              "width": { "type": "integer", "default": 1024 },
              "height": { "type": "integer", "default": 1024 },
              "seed": { "type": "integer" }
            },
            "required": ["prompt"]
          },
          "outputs": ["image"],
          "execution": "server_side",
          "approval": "confirm"
        }
      ]
    }
  ]
}
```

## 10. Agent Outputs

### 10.1 Chat Reply

```json
{
  "message": "你好，我是 Helixflow workflow agent，可以通过聊天帮你设计、修改和运行工作流。"
}
```

### 10.2 Graph Proposal

```json
{
  "base_version_id": "ver_...",
  "kind": "create",
  "title": "Text to image workflow",
  "summary": "Creates a catalog-backed text-to-image workflow.",
  "ops": []
}
```

Proposal rules:

- proposal is a diff, not a full replacement graph;
- backend validates node types, params, ports, and edges;
- backend rejects unknown fields;
- backend rejects stale `base_version_id`;
- backend persists a preview graph before showing it.

### 10.3 Clarification Form

```json
{
  "type": "clarification_form",
  "title": "Choose runtime provider",
  "questions": [
    {
      "id": "runtime_provider",
      "type": "single_select",
      "options": [
        { "value": "atlas", "label": "Atlas" },
        { "value": "local_comfyui", "label": "Local ComfyUI" }
      ]
    }
  ]
}
```

### 10.4 Run Request

```json
{
  "type": "run_request",
  "base_version_id": "ver_...",
  "runtime_provider_id": "atlas",
  "estimated_cost": {
    "amount": 0.03,
    "currency": "USD"
  },
  "requires_confirmation": true,
  "reason": "This run uses a hosted image generation API."
}
```

## 11. Data Model Additions

Recommended tables:

```sql
CREATE TABLE prompt_turns (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  message_id TEXT,
  mode TEXT NOT NULL,
  output_contract TEXT NOT NULL,
  prompt_fingerprint TEXT NOT NULL,
  stack_fingerprint TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE prompt_sections (
  id TEXT PRIMARY KEY,
  turn_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  section_key TEXT NOT NULL,
  present INTEGER NOT NULL,
  raw_bytes INTEGER NOT NULL,
  redacted_bytes INTEGER NOT NULL,
  fingerprint TEXT NOT NULL,
  redacted_content TEXT,
  metadata_json TEXT,
  FOREIGN KEY(turn_id) REFERENCES prompt_turns(id)
);

CREATE TABLE runtime_providers (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  kind TEXT NOT NULL,
  config_json TEXT NOT NULL,
  enabled INTEGER NOT NULL
);

CREATE TABLE api_connectors (
  id TEXT PRIMARY KEY,
  runtime_provider_id TEXT NOT NULL,
  capability_json TEXT NOT NULL,
  enabled INTEGER NOT NULL
);

CREATE TABLE capability_grants (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  run_id TEXT,
  provider_id TEXT NOT NULL,
  connector_id TEXT,
  operations_json TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  revoked_at INTEGER
);
```

## 12. API Surface

### 12.1 Workbench State

```http
GET /api/workspaces/{workspace_id}/state
```

State should include:

- current graph;
- pending proposal;
- run status;
- chat messages;
- prompt debug availability;
- runtime provider summary;
- API connector summary.

### 12.2 Prompt Debug

```http
GET /api/workspaces/{workspace_id}/agent-turns/{turn_id}/prompt
```

Returns redacted prompt telemetry, not raw secrets.

### 12.3 Provider Catalog

```http
GET /api/workspaces/{workspace_id}/runtime/catalog
```

Returns workflow backends, runtime providers, and API connectors.

### 12.4 Run Confirmation

```http
POST /api/workspaces/{workspace_id}/runs/{run_request_id}/approve
POST /api/workspaces/{workspace_id}/runs/{run_request_id}/reject
```

## 13. Transcript Rendering

UI rules:

- user messages render as user chat bubbles;
- assistant replies render as assistant chat bubbles;
- pending proposals render as assistant proposal cards;
- tool logs are nested under the assistant turn;
- lifecycle events are hidden by default;
- expected file writes are hidden by default;
- command start/result pairs are deduplicated;
- raw event JSON remains available behind debug expansion.

## 14. Error Handling

Agent errors:

- invalid JSON output;
- output file missing;
- unknown proposal fields;
- stale base version;
- invalid node type/param/edge;
- prompt exceeded runtime budget;
- runtime provider not selected;
- connector unavailable.

Provider errors:

- auth missing;
- rate limit;
- timeout;
- provider task failed;
- malformed output;
- cost limit exceeded.

Each error should map to either:

- assistant reply;
- fix proposal;
- run error message;
- structured confirmation/clarification.

## 15. Prompt Telemetry

Prompt telemetry should store:

- section order;
- presence;
- raw byte size;
- redacted byte size;
- section fingerprint;
- prompt fingerprint;
- stack fingerprint;
- truncation reason;
- redacted content for safe sections;
- metadata for sensitive sections.

Telemetry must redact:

- local filesystem paths;
- provider secrets;
- API keys;
- signed URLs;
- capability token values;
- environment variables.

## 16. Prompt Budget

Runtime adapters should declare:

```rust
pub struct RuntimePromptBudget {
    pub max_prompt_bytes: Option<usize>,
    pub supports_stdin_prompt: bool,
    pub supports_resume: bool,
}
```

If a prompt exceeds adapter limits:

- return `AGENT_PROMPT_TOO_LARGE`;
- report section sizes;
- suggest disabling atoms or reducing context;
- do not silently truncate required output contracts.

## 17. Tests

### 17.1 Prompt Composer Tests

- chat prompt forbids ctx reads and shell commands;
- create prompt includes graph and catalogs;
- modify prompt requires smallest diff;
- runtime provider prompt does not hardcode Atlas;
- ComfyUI appears only as workflow backend/adapter;
- prompt section order is stable;
- echo guard is present;
- telemetry redacts local paths and secrets.

### 17.2 Router Tests

- `你好` -> `Chat`;
- `你是谁` -> `Chat`;
- `帮我做一个图生图工作流` -> `CreateWorkflow`;
- empty graph plus workflow request -> `CreateWorkflow`;
- non-empty graph plus `加一个 ControlNet` -> `ModifyWorkflow`;
- `运行这个 workflow` -> `RunRequest`;
- provider selection request -> `ClarificationForm` if ambiguous.

### 17.3 Provider Tests

- runtime catalog exposes arbitrary provider ids;
- Atlas connector works without special prompt code;
- disabled connector is not exposed to agent;
- missing credentials prevent run request approval;
- cost gate blocks paid run until user approval;
- backend rejects invented provider ids.

### 17.4 UI Tests

- plain chat renders without tool group when no visible tools ran;
- proposal renders as chat review card;
- lifecycle JSON is hidden by default;
- prompt debug opens redacted section list;
- selected node comment scopes modification to target node/subgraph.

## 18. Rollout Plan

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
- require confirmation for paid/external runs;
- log provider run evidence.

### Phase 4: Workflow Atoms

- add atom files under `ctx/atoms`;
- inject atoms by route/mode;
- add tests for atom selection.

### Phase 5: Prompt Debug UI

- expose redacted prompt telemetry endpoint;
- add "查看 prompt" debug affordance;
- keep normal chat clean.

## 19. Acceptance Criteria

1. Empty workspace has no graph until chat creates one.
2. Plain chat still calls the agent but does not read ctx or run shell.
3. Workflow creation produces a validated pending proposal.
4. Workflow modification preserves existing graph unless change is required.
5. ComfyUI is represented as workflow backend/adapter.
6. Atlas is represented as runtime provider and/or API connector data.
7. Arbitrary future APIs can be registered without prompt changes.
8. Provider execution never uses mock output.
9. Provider execution never exposes credentials to the agent.
10. Prompt sections are stored with redacted telemetry.
11. UI shows chat first, proposals as cards, and logs as nested evidence.

