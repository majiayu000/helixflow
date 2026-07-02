use std::fs;
use std::path::{Component, Path, PathBuf};

use helixflow_graph::{GraphService, ProposalDraft, ProposalKind, ProposalOp, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    AgentError, AgentResult, AgentSession, AgentSessionRequest, AgentSkill, ValidatedAgentProposal,
    ValidatedDesignArtifact,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalOutput {
    base_version_id: String,
    kind: ProposalKind,
    title: String,
    summary: String,
    ops: Vec<ProposalOp>,
    #[serde(default)]
    message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignArtifactOutput {
    manifest_version: u64,
    title: String,
    summary: String,
    kind: String,
    entry_file: String,
    mime: String,
    #[serde(default)]
    meta: Value,
}

impl From<ProposalOutput> for ProposalDraft {
    fn from(value: ProposalOutput) -> Self {
        Self {
            base_version_id: value.base_version_id,
            kind: value.kind,
            title: value.title,
            summary: value.summary,
            ops: value.ops,
            message_id: value.message_id,
        }
    }
}

pub fn create_session_contract(request: &AgentSessionRequest) -> AgentResult<AgentSession> {
    let session_id = format!("agent_{}", Uuid::now_v7().simple());
    let root_dir = request.sessions_dir.join(&session_id);
    let ctx_dir = root_dir.join("ctx");
    let out_dir = root_dir.join("out");
    let tmp_dir = root_dir.join("tmp");
    let skills_dir = ctx_dir.join("skills");
    let node_defs_dir = ctx_dir.join("node_defs");

    fs::create_dir_all(&skills_dir)?;
    fs::create_dir_all(&node_defs_dir)?;
    fs::create_dir_all(&out_dir)?;
    fs::create_dir_all(&tmp_dir)?;

    write_json(ctx_dir.join("graph.json"), &request.graph)?;
    fs::write(ctx_dir.join("project.md"), project_markdown(request))?;
    fs::write(ctx_dir.join("design_system.md"), design_system_markdown())?;
    write_json(
        node_defs_dir.join("catalog.json"),
        &NodeRegistry::builtin().export_catalog(),
    )?;
    fs::write(
        ctx_dir.join("instructions.md"),
        instructions_markdown(request),
    )?;
    fs::write(skills_dir.join("node_library.md"), node_library_skill())?;
    fs::write(
        skills_dir.join(request.skill.file_name()),
        selected_skill(request.skill),
    )?;
    fs::write(
        root_dir.join("transcript.jsonl"),
        format!(
            "{}\n",
            serde_json::json!({
                "role": "user",
                "message": request.user_message,
                "skill": request.skill
            })
        ),
    )?;

    Ok(AgentSession {
        id: session_id,
        workspace_id: request.workspace_id.clone(),
        root_dir,
        ctx_dir,
        out_dir,
        base_version_id: request.base_version_id.clone(),
    })
}

pub fn read_validated_proposal(
    session: &AgentSession,
    base_graph: &WorkflowGraph,
    current_version_id: &str,
) -> AgentResult<ValidatedAgentProposal> {
    let output_path = session.out_dir.join("proposal.json");
    let output: ProposalOutput =
        serde_json::from_slice(&read_output_file(&session.out_dir, &output_path)?)?;
    let proposal = GraphService::new(NodeRegistry::builtin()).preview_proposal(
        base_graph,
        current_version_id,
        output.into(),
    )?;

    Ok(ValidatedAgentProposal {
        session_id: session.id.clone(),
        proposal,
    })
}

pub fn read_validated_design_artifact(
    session: &AgentSession,
) -> AgentResult<ValidatedDesignArtifact> {
    let manifest_path = session.out_dir.join("artifact.json");
    let output: DesignArtifactOutput =
        serde_json::from_slice(&read_output_file(&session.out_dir, &manifest_path)?)?;
    validate_design_artifact_manifest(session, output)
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> AgentResult<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn instructions_markdown(request: &AgentSessionRequest) -> String {
    if request.skill == AgentSkill::Chat {
        return format!(
            r#"
# Helixflow Chat Turn

Workspace: {workspace_id}
Base version: {base_version_id}

This is a plain chat turn. Answer the user's question directly.
Do not inspect files or run shell commands for greetings, identity questions, or general chat.
If the user asks to create or modify a workflow, ask for the missing concrete details instead of changing the graph.
Write exactly one result file: `out/reply.json`.
Schema: {{"message":"your answer"}}.
Do not write credentials, local filesystem paths, `proposal.json`, or files outside `out/`.

User request:
{user_message}
"#,
            workspace_id = request.workspace_id,
            base_version_id = request.base_version_id,
            user_message = request.user_message
        );
    }
    if request.skill == AgentSkill::DesignArtifact {
        return format!(
            r#"
# Helixflow Design Artifact Turn

Workspace: {workspace_id}
Base version: {base_version_id}

You are designing a workspace artifact for the user, not filling a fixed workflow graph.
Read `ctx/project.md`, `ctx/design_system.md`, `ctx/graph.json`, and `ctx/skills/design_artifact.md`.
Use `ctx/graph.json` only as optional context. If it is empty, start from the user's brief.

Write exactly one manifest file: `out/artifact.json`.
Write artifact files only under `out/files/`.
Do not write `proposal.json` unless the user explicitly asks for a graph workflow.
Do not write credentials, local filesystem paths, mock provider outputs, or files outside `out/`.

Required manifest schema:
{{"manifest_version":1,"title":"artifact title","summary":"what was created","kind":"html","entry_file":"files/index.html","mime":"text/html","meta":{{}}}}

Rules:
- `entry_file` must be a relative path inside `files/`.
- For UI, app, page, dashboard, or design requests, create a self-contained HTML artifact.
- Put CSS and JS inline unless another local file is necessary.
- The result must be useful to preview immediately; do not return placeholder text.

User request:
{user_message}
"#,
            workspace_id = request.workspace_id,
            base_version_id = request.base_version_id,
            user_message = request.user_message
        );
    }

    let proposal_kind = match request.skill {
        AgentSkill::CreateWorkflow => "create",
        AgentSkill::FixError => "fix",
        AgentSkill::Sweep => "sweep",
        AgentSkill::Chat | AgentSkill::DesignArtifact | AgentSkill::ModifyWorkflow => "modify",
    };
    format!(
        r#"
# Helixflow Agent Turn

Workspace: {workspace_id}
Base version: {base_version_id}

Read `ctx/graph.json` and `ctx/node_defs/catalog.json`.
Write exactly one result file under `out/`.
For graph changes, write `out/proposal.json` matching the proposal schema.
Required proposal fields: base_version_id={base_version_id}, kind={proposal_kind}, title, summary, ops, optional message_id.
Allowed op JSON shapes:
- add_node: {{"op":"add_node","id":"new_node_id","node":{{"node_type":"input.image","title":"Image Input","params":{{"storage_uri":"workspace://uploads/example.png"}},"pos":[80.0,320.0]}}}}
- remove_node: {{"op":"remove_node","id":"node_id"}}
- set_param: {{"op":"set_param","id":"node_id","key":"param_name","prev":"old value","value":"new value"}}
- add_edge: {{"op":"add_edge","edge":{{"from":["from_node_id","output_port"],"to":["to_node_id","input_port"],"edge_type":"artifact"}}}}
- remove_edge: {{"op":"remove_edge","edge":{{"from":["from_node_id","output_port"],"to":["to_node_id","input_port"],"edge_type":"artifact"}}}}
- move_node: {{"op":"move_node","id":"node_id","pos":[120.0,240.0]}}
For add_node, put node_type, title, params, and pos inside the nested `node` object.
Use only node types, params, and ports from `ctx/node_defs/catalog.json`.
Use existing node ids from `ctx/graph.json` when editing existing nodes or preserving connections.
Do not write credentials or local filesystem paths.

User request:
{user_message}
"#,
        workspace_id = request.workspace_id,
        base_version_id = request.base_version_id,
        proposal_kind = proposal_kind,
        user_message = request.user_message
    )
}

fn node_library_skill() -> &'static str {
    "# Node Library\nUse built-in node definitions from ctx/node_defs/catalog.json.\n"
}

fn selected_skill(skill: AgentSkill) -> &'static str {
    match skill {
        AgentSkill::Chat => "# Chat\nAnswer in out/reply.json without changing the graph.\n",
        AgentSkill::DesignArtifact => {
            "# Design Artifact\nCreate a real artifact from the user brief. Write out/artifact.json and files under out/files/.\n"
        }
        AgentSkill::CreateWorkflow => "# Create Workflow\nCreate a valid graph proposal.\n",
        AgentSkill::ModifyWorkflow => "# Modify Workflow\nMake the smallest valid proposal.\n",
        AgentSkill::FixError => "# Fix Error\nPropose the smallest graph fix.\n",
        AgentSkill::Sweep => "# Sweep\nWrite a sweep run plan when requested.\n",
    }
}

fn project_markdown(request: &AgentSessionRequest) -> String {
    format!(
        r#"# Helixflow Project Context

Workspace: {workspace_id}
Base version: {base_version_id}

Helixflow is an agentic design and workflow workspace. A user can start from an empty
workspace, describe the desired output, and receive a concrete artifact before any
workflow graph exists. Graph workflows are still available when the user explicitly
asks for nodes, parameters, connections, or provider execution.
"#,
        workspace_id = request.workspace_id,
        base_version_id = request.base_version_id
    )
}

fn design_system_markdown() -> &'static str {
    r#"# Design System

Use a restrained product-tool interface by default: dense, legible, responsive, and
ready for repeated use. Prefer clear hierarchy, stable dimensions, accessible
contrast, and concrete preview content over marketing filler.
"#
}

fn validate_design_artifact_manifest(
    session: &AgentSession,
    output: DesignArtifactOutput,
) -> AgentResult<ValidatedDesignArtifact> {
    if output.manifest_version != 1 {
        return Err(AgentError::InvalidOutputFile {
            path: session.out_dir.join("artifact.json"),
            reason: "artifact manifest_version must be 1".to_owned(),
        });
    }
    let title = non_empty_field(
        "title",
        output.title,
        &session.out_dir.join("artifact.json"),
    )?;
    let summary = non_empty_field(
        "summary",
        output.summary,
        &session.out_dir.join("artifact.json"),
    )?;
    let kind = normalize_artifact_kind(&output.kind, &session.out_dir.join("artifact.json"))?;
    let mime = non_empty_field("mime", output.mime, &session.out_dir.join("artifact.json"))?;
    let (files_dir, entry_path) = validated_entry_path(session, &output.entry_file)?;

    Ok(ValidatedDesignArtifact {
        session_id: session.id.clone(),
        title,
        summary,
        kind,
        entry_file: output.entry_file,
        entry_path,
        files_dir,
        mime,
        meta: output.meta,
    })
}

fn non_empty_field(field: &str, value: String, path: &Path) -> AgentResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AgentError::InvalidOutputFile {
            path: path.to_path_buf(),
            reason: format!("{field} must not be empty"),
        });
    }
    Ok(trimmed.to_owned())
}

fn normalize_artifact_kind(kind: &str, path: &Path) -> AgentResult<String> {
    let normalized = kind.trim().to_ascii_lowercase();
    let allowed = ["html", "markdown", "text", "json", "image", "video"];
    if allowed.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(AgentError::InvalidOutputFile {
            path: path.to_path_buf(),
            reason: format!("unsupported artifact kind `{kind}`"),
        })
    }
}

fn validated_entry_path(
    session: &AgentSession,
    entry_file: &str,
) -> AgentResult<(PathBuf, PathBuf)> {
    let entry = Path::new(entry_file);
    if entry.is_absolute()
        || entry.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || !entry.starts_with("files")
    {
        return Err(AgentError::PathOutsideSession(entry.to_path_buf()));
    }
    let candidate = session.out_dir.join(entry);
    let files_dir = session.out_dir.join("files");
    let files_type = fs::symlink_metadata(&files_dir)?.file_type();
    if files_type.is_symlink() || !files_type.is_dir() {
        return Err(AgentError::InvalidOutputFile {
            path: files_dir,
            reason: "files directory is not a real directory".to_owned(),
        });
    }
    let candidate_type = fs::symlink_metadata(&candidate)?.file_type();
    if candidate_type.is_symlink() || !candidate_type.is_file() {
        return Err(AgentError::InvalidOutputFile {
            path: candidate,
            reason: "artifact entry file is not a regular file".to_owned(),
        });
    }
    let canonical_files = fs::canonicalize(&files_dir)?;
    let canonical_candidate = fs::canonicalize(&candidate)?;
    if !canonical_candidate.starts_with(&canonical_files) {
        return Err(AgentError::PathOutsideSession(candidate));
    }
    Ok((canonical_files, canonical_candidate))
}

fn ensure_direct_child(parent: &Path, candidate: &Path) -> AgentResult<()> {
    if candidate.parent() == Some(parent) {
        Ok(())
    } else {
        Err(AgentError::PathOutsideSession(candidate.to_path_buf()))
    }
}

pub(crate) fn read_output_file(out_dir: &Path, output_path: &Path) -> AgentResult<Vec<u8>> {
    ensure_direct_child(out_dir, output_path)?;

    let out_type = fs::symlink_metadata(out_dir)?.file_type();
    if out_type.is_symlink() || !out_type.is_dir() {
        return Err(AgentError::InvalidOutputFile {
            path: out_dir.to_path_buf(),
            reason: "out directory is not a real directory".to_owned(),
        });
    }

    let output_type = fs::symlink_metadata(output_path)?.file_type();
    if output_type.is_symlink() || !output_type.is_file() {
        return Err(AgentError::InvalidOutputFile {
            path: output_path.to_path_buf(),
            reason: "agent output is not a regular file".to_owned(),
        });
    }

    let canonical_out = fs::canonicalize(out_dir)?;
    let canonical_output = fs::canonicalize(output_path)?;
    if canonical_output.parent() != Some(canonical_out.as_path()) {
        return Err(AgentError::PathOutsideSession(output_path.to_path_buf()));
    }

    Ok(fs::read(canonical_output)?)
}
