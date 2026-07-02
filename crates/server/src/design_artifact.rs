use std::fs;
use std::path::{Component, Path};

use helixflow_agent::{AgentSessionRequest, AgentSkill, ValidatedDesignArtifact};
use helixflow_graph::WorkflowGraph;
use helixflow_store::{NewArtifact, NewRun};
use serde_json::{Value, json};

use crate::agent_transcript::read_agent_messages;
use crate::workbench::{Workbench, WorkbenchError, WorkbenchResult};

pub(crate) fn should_create_design_artifact(text: &str) -> bool {
    let normalized = text.trim().to_lowercase();
    if normalized.is_empty() || has_graph_keyword(&normalized) {
        return false;
    }
    explicit_artifact_keywords()
        .iter()
        .any(|keyword| normalized.contains(keyword))
}

impl Workbench {
    pub(crate) async fn create_design_artifact_from_message(
        &self,
        workspace_id: &str,
        base_version_id: &str,
        graph: WorkflowGraph,
        text: &str,
        user_message_id: &str,
    ) -> WorkbenchResult<Value> {
        let artifact = match self
            .agent
            .create_design_artifact(AgentSessionRequest {
                workspace_id: workspace_id.to_owned(),
                base_version_id: base_version_id.to_owned(),
                user_message: text.to_owned(),
                graph,
                sessions_dir: self.data_dir.join("agent-sessions"),
                skill: AgentSkill::DesignArtifact,
            })
            .await
        {
            Ok(artifact) => artifact,
            Err(err) => {
                let message = format!("Agent artifact generation failed: {err}");
                self.create_message(workspace_id, "system", &message, "error", None)
                    .await?;
                return Err(WorkbenchError::Agent(message));
            }
        };

        for message in read_agent_messages(&self.data_dir, &artifact.session_id)? {
            let kind = format!("agent_log:{}", message.kind);
            self.create_message(
                workspace_id,
                "agent",
                &message.text,
                &kind,
                Some(&message.label),
            )
            .await?;
        }

        let plan_json = json!({
            "kind": "design_artifact",
            "sessionId": artifact.session_id,
            "entryFile": artifact.entry_file
        })
        .to_string();
        let run = self
            .store
            .create_run(NewRun {
                workspace_id,
                version_id: base_version_id,
                group_id: None,
                label: &artifact.title,
                trigger: "agent",
                plan_json: Some(&plan_json),
                estimate_json: None,
                status: "running",
            })
            .await?;
        let storage_uri = persist_design_artifact(&self.data_dir, workspace_id, &artifact)?;
        let meta_json = artifact_meta_json(&artifact, user_message_id);
        self.store
            .create_artifact(NewArtifact {
                workspace_id,
                run_id: Some(&run.id),
                run_step_id: None,
                node_id: None,
                kind: &artifact.kind,
                storage_uri: &storage_uri,
                sha256: None,
                mime: Some(&artifact.mime),
                width: None,
                height: None,
                duration_ms: None,
                selected: true,
                meta_json: Some(&meta_json),
            })
            .await?;
        self.store
            .update_run_status(&run.id, "succeeded", None)
            .await?;
        self.create_message(
            workspace_id,
            "agent",
            &format!("已生成 artifact：{}。{}", artifact.title, artifact.summary),
            "artifact_created",
            Some(&artifact.session_id),
        )
        .await?;
        self.state(workspace_id).await
    }
}

fn persist_design_artifact(
    data_dir: &Path,
    workspace_id: &str,
    artifact: &ValidatedDesignArtifact,
) -> WorkbenchResult<String> {
    let target_root = data_dir
        .join("workspaces")
        .join(workspace_id)
        .join("artifacts")
        .join(&artifact.session_id);
    if target_root.exists() {
        fs::remove_dir_all(&target_root)?;
    }
    copy_dir_contents(&artifact.files_dir, &target_root)?;

    let entry_rel = Path::new(&artifact.entry_file)
        .strip_prefix("files")
        .map_err(|_| {
            WorkbenchError::BadRequest("artifact entry_file must be inside files/".to_owned())
        })?;
    let entry_storage = normalize_path_for_uri(entry_rel)?;
    Ok(format!(
        "workspace://workspaces/{workspace_id}/artifacts/{}/{}",
        artifact.session_id, entry_storage
    ))
}

fn copy_dir_contents(source: &Path, target: &Path) -> WorkbenchResult<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = fs::symlink_metadata(&source_path)?.file_type();
        if file_type.is_symlink() {
            return Err(WorkbenchError::BadRequest(format!(
                "artifact files cannot contain symlinks: {}",
                source_path.display()
            )));
        }
        if file_type.is_dir() {
            copy_dir_contents(&source_path, &target_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, target_path)?;
        }
    }
    Ok(())
}

fn artifact_meta_json(artifact: &ValidatedDesignArtifact, user_message_id: &str) -> String {
    json!({
        "title": artifact.title,
        "summary": artifact.summary,
        "entryFile": artifact.entry_file,
        "sessionId": artifact.session_id,
        "source": "agent_design_artifact",
        "userMessageId": user_message_id,
        "agentMeta": artifact.meta
    })
    .to_string()
}

fn normalize_path_for_uri(path: &Path) -> WorkbenchResult<String> {
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(WorkbenchError::BadRequest(format!(
            "artifact path is not relative: {}",
            path.display()
        )));
    }
    let text = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if text.is_empty() {
        return Err(WorkbenchError::BadRequest(
            "artifact entry path is empty".to_owned(),
        ));
    }
    Ok(text)
}

fn has_graph_keyword(text: &str) -> bool {
    graph_keywords()
        .iter()
        .any(|keyword| text.contains(keyword))
}

fn graph_keywords() -> &'static [&'static str] {
    &[
        "workflow",
        "工作流",
        "node",
        "节点",
        "graph",
        "comfy",
        "comfyui",
        "controlnet",
        "seed",
        "queue",
        "连接",
    ]
}

fn explicit_artifact_keywords() -> &'static [&'static str] {
    &[
        "html artifact",
        "html 文件",
        "html file",
        "静态 html",
        "静态页面",
        "网页原型",
        "页面原型",
        "artifact 文件",
        "artifact file",
        "生成 artifact",
        "输出 artifact",
        "导出 html",
        "可直接打开的 html",
    ]
}

#[cfg(test)]
mod tests {
    use super::should_create_design_artifact;

    #[test]
    fn dashboard_status_copy_stays_on_graph_path() {
        assert!(!should_create_design_artifact(
            "设计一个任务运行监控 dashboard，包含运行状态和 artifact 预览"
        ));
    }

    #[test]
    fn explicit_html_artifact_copy_uses_artifact_path() {
        assert!(should_create_design_artifact(
            "生成一个可直接打开的 HTML artifact 文件"
        ));
    }

    #[test]
    fn explicit_workflow_copy_stays_on_graph_path() {
        assert!(!should_create_design_artifact(
            "创建一个 ComfyUI 文生图 workflow，包含节点、参数和连接"
        ));
    }
}
