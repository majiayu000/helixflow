use super::*;
use crate::{NewVersion, VersionSource};

async fn open_temp_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("helixflow.sqlite");
    let database_url = format!("sqlite://{}", db_path.display());
    let store = Store::open(&database_url).await.expect("open store");
    (store, dir)
}

#[tokio::test]
async fn select_run_artifact_clears_sibling_artifacts() {
    let (store, _dir) = open_temp_store().await;
    let workspace = store
        .create_workspace("Artifacts")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Graph",
            source: VersionSource::Manual,
            graph_path: "graphs/current.json",
            graph_hash: "sha256:graph",
            parent_id: None,
        })
        .await
        .expect("create version");
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace.id,
            version_id: &version.id,
            group_id: None,
            label: "Run",
            trigger: "manual",
            plan_json: None,
            estimate_json: None,
            status: "succeeded",
        })
        .await
        .expect("create run");
    let first = artifact(&store, &workspace.id, &run.id, "first", true).await;
    let second = artifact(&store, &workspace.id, &run.id, "second", false).await;

    let selected = store
        .select_run_artifact(&second.id)
        .await
        .expect("select artifact");
    let artifacts = store.run_artifacts(&run.id).await.expect("artifacts");

    assert_eq!(selected.id, second.id);
    assert_eq!(
        artifacts
            .iter()
            .filter(|artifact| artifact.selected)
            .map(|artifact| artifact.id.as_str())
            .collect::<Vec<_>>(),
        vec![second.id.as_str()]
    );
    assert!(!store.artifact(&first.id).await.expect("first").selected);
}

async fn artifact(
    store: &Store,
    workspace_id: &str,
    run_id: &str,
    node_id: &str,
    selected: bool,
) -> ArtifactRecord {
    store
        .create_artifact(NewArtifact {
            workspace_id,
            run_id: Some(run_id),
            run_step_id: None,
            node_id: Some(node_id),
            kind: "video",
            storage_uri: "workspace://outputs/run/video.mp4",
            sha256: None,
            mime: Some("video/mp4"),
            width: Some(1080),
            height: Some(1920),
            duration_ms: Some(5000),
            selected,
            meta_json: Some(r#"{"provider":"mock","capability":"text_to_video"}"#),
        })
        .await
        .expect("create artifact")
}
