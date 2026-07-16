use super::{NewProposal, NewVersion, Store, VersionSource};

async fn open_temp_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    (store, dir)
}

#[tokio::test]
async fn version_file_references_return_every_exact_path_match() {
    let (store, _dir) = open_temp_store().await;
    let workspace = store
        .create_workspace("Reference workspace")
        .await
        .expect("create workspace");
    let first = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "First",
            source: VersionSource::Manual,
            graph_path: "graphs/shared.json",
            graph_hash: "sha256:first",
            parent_id: None,
        })
        .await
        .expect("create first version");
    let second = store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace.id,
                label: "Second",
                source: VersionSource::Manual,
                graph_path: "graphs/shared.json",
                graph_hash: "sha256:second",
                parent_id: Some(&first.id),
            },
            &first.id,
        )
        .await
        .expect("create second version");

    let references = store
        .version_file_references("graphs/shared.json")
        .await
        .expect("version references");

    assert_eq!(
        references
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.id.as_str(), second.id.as_str()]
    );
    assert!(
        store
            .version_file_references("graphs/missing.json")
            .await
            .expect("missing references")
            .is_empty()
    );
}

#[tokio::test]
async fn proposal_file_references_match_ops_or_preview_path_exactly() {
    let (store, _dir) = open_temp_store().await;
    let workspace = store
        .create_workspace("Proposal reference workspace")
        .await
        .expect("create workspace");
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: "graphs/base.json",
            graph_hash: "sha256:base",
            parent_id: None,
        })
        .await
        .expect("create base");
    let proposal = store
        .create_proposal(NewProposal {
            workspace_id: &workspace.id,
            base_version_id: &base.id,
            kind: "modify",
            title: "Reference paths",
            summary: "Reference paths",
            ops_path: "proposals/reference/ops.json",
            preview_graph_path: Some("proposals/reference/preview.json"),
            message_id: None,
        })
        .await
        .expect("create proposal");

    let ops_references = store
        .proposal_file_references("proposals/reference/ops.json")
        .await
        .expect("ops references");
    let preview_references = store
        .proposal_file_references("proposals/reference/preview.json")
        .await
        .expect("preview references");

    assert_eq!(ops_references.len(), 1);
    assert_eq!(ops_references[0].proposal_id, proposal.id);
    assert_eq!(preview_references, ops_references);
    assert!(
        store
            .proposal_file_references("proposals/reference/missing.json")
            .await
            .expect("missing references")
            .is_empty()
    );
}
