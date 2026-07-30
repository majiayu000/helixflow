use tempfile::tempdir;

use super::{
    AgentContractEvidenceFilter, AgentContractMode, AgentContractOutcome,
    AutoApplyProposalVersionRecord, CompleteAgentContractObservation, NewAgentContractObservation,
    NewMessage, NewProposal, NewVersion, Store, StoreError, VersionSource,
};

async fn fixture() -> (Store, String) {
    let dir = tempdir().expect("tempdir");
    let path = dir.keep().join("store.sqlite");
    let store = Store::open(&format!("sqlite://{}", path.display()))
        .await
        .expect("open store");
    let workspace = store
        .create_workspace("Contract evidence")
        .await
        .expect("workspace");
    (store, workspace.id)
}

fn user_message<'a>(workspace_id: &'a str, text: &'a str) -> NewMessage<'a> {
    NewMessage {
        workspace_id,
        role: "user",
        kind: "text",
        text: Some(text),
        ref_id: None,
        attachment_ids_json: Some(r#"{"turnMode":"create_workflow"}"#),
    }
}

#[tokio::test]
async fn graph_edit_message_and_started_observation_are_atomic() {
    let (store, workspace_id) = fixture().await;
    let started = store
        .create_graph_edit_message_with_observation(NewAgentContractObservation {
            user_message: user_message(&workspace_id, "make an image"),
            contract_mode: AgentContractMode::Intent,
            release_id: Some("v0.2.0"),
            build_revision: Some("abc123"),
        })
        .await
        .expect("started");

    assert_eq!(started.observation.user_message_id, started.message.id);
    assert_eq!(started.observation.workspace_id, workspace_id);
    assert_eq!(started.observation.contract_mode, "intent");
    assert_eq!(started.observation.outcome, "started");
    assert_eq!(started.observation.release_id.as_deref(), Some("v0.2.0"));
    assert_eq!(
        started.observation.build_revision.as_deref(),
        Some("abc123")
    );
    assert!(started.observation.reason_code.is_none());
    assert!(started.observation.completed_at.is_none());

    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(store.pool())
        .await
        .expect("message count");
    let error = store
        .create_graph_edit_message_with_observation(NewAgentContractObservation {
            user_message: user_message("missing_workspace", "must roll back"),
            contract_mode: AgentContractMode::Intent,
            release_id: None,
            build_revision: None,
        })
        .await
        .expect_err("foreign key");
    assert!(matches!(error, StoreError::Sqlx(_)));
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
        .fetch_one(store.pool())
        .await
        .expect("message count");
    assert_eq!(
        before, after,
        "failed observation must roll back its message"
    );
}

#[tokio::test]
async fn terminal_completion_is_idempotent_and_conflicts_fail_closed() {
    let (store, workspace_id) = fixture().await;
    let started = store
        .create_graph_edit_message_with_observation(NewAgentContractObservation {
            user_message: user_message(&workspace_id, "make an image"),
            contract_mode: AgentContractMode::Intent,
            release_id: None,
            build_revision: None,
        })
        .await
        .expect("started");
    let completion = CompleteAgentContractObservation {
        observation_id: &started.observation.id,
        workspace_id: &workspace_id,
        contract_mode: AgentContractMode::Intent,
        outcome: AgentContractOutcome::Success,
        reason_code: "INTENT_COMPILED",
        session_id: Some("agent_1"),
    };
    let completed = store
        .finalize_agent_contract_observation(completion.clone())
        .await
        .expect("complete");
    let replayed = store
        .finalize_agent_contract_observation(completion)
        .await
        .expect("replay");
    assert_eq!(completed, replayed);
    assert_eq!(completed.outcome, "success");
    assert_eq!(completed.reason_code.as_deref(), Some("INTENT_COMPILED"));

    let error = store
        .finalize_agent_contract_observation(CompleteAgentContractObservation {
            observation_id: &started.observation.id,
            workspace_id: &workspace_id,
            contract_mode: AgentContractMode::Intent,
            outcome: AgentContractOutcome::Error,
            reason_code: "INTENT_COMPILE_ERROR",
            session_id: Some("agent_1"),
        })
        .await
        .expect_err("conflicting completion");
    assert!(matches!(
        error,
        StoreError::AgentContractObservationInvariant {
            code: "TERMINAL_COMPLETION_CONFLICT"
        }
    ));
}

#[tokio::test]
async fn clarification_message_and_terminal_observation_commit_together() {
    let (store, workspace_id) = fixture().await;
    let started = store
        .create_graph_edit_message_with_observation(NewAgentContractObservation {
            user_message: user_message(&workspace_id, "use a model"),
            contract_mode: AgentContractMode::Intent,
            release_id: Some("v0.2.0"),
            build_revision: Some("abc123"),
        })
        .await
        .expect("started");
    let clarified = store
        .create_clarification_and_finalize_observation(
            NewMessage {
                workspace_id: &workspace_id,
                role: "agent",
                kind: "clarify",
                text: Some("需要澄清 [MODEL_AMBIGUOUS]"),
                ref_id: Some("agent_2"),
                attachment_ids_json: None,
            },
            CompleteAgentContractObservation {
                observation_id: &started.observation.id,
                workspace_id: &workspace_id,
                contract_mode: AgentContractMode::Intent,
                outcome: AgentContractOutcome::Clarify,
                reason_code: "MODEL_AMBIGUOUS",
                session_id: Some("agent_2"),
            },
        )
        .await
        .expect("clarify");

    assert_eq!(clarified.message.kind, "clarify");
    assert_eq!(clarified.observation.outcome, "clarify");
    assert_eq!(
        clarified.observation.reason_code.as_deref(),
        Some("MODEL_AMBIGUOUS")
    );
}

#[tokio::test]
async fn startup_finalizes_only_inflight_observations_once() {
    let (store, workspace_id) = fixture().await;
    let started = store
        .create_graph_edit_message_with_observation(NewAgentContractObservation {
            user_message: user_message(&workspace_id, "unfinished"),
            contract_mode: AgentContractMode::Legacy,
            release_id: None,
            build_revision: None,
        })
        .await
        .expect("started");

    assert_eq!(
        store
            .finalize_interrupted_agent_contract_observations()
            .await
            .expect("finalize"),
        1
    );
    assert_eq!(
        store
            .finalize_interrupted_agent_contract_observations()
            .await
            .expect("replay"),
        0
    );
    let completed = store
        .agent_contract_observation(&started.observation.id)
        .await
        .expect("observation");
    assert_eq!(completed.outcome, "error");
    assert_eq!(
        completed.reason_code.as_deref(),
        Some("PROCESS_INTERRUPTED")
    );
}

#[tokio::test]
async fn identities_and_reason_codes_reject_sensitive_or_free_form_values() {
    let (store, workspace_id) = fixture().await;
    let error = store
        .create_graph_edit_message_with_observation(NewAgentContractObservation {
            user_message: user_message(&workspace_id, "private"),
            contract_mode: AgentContractMode::Intent,
            release_id: Some("https://release.invalid/token"),
            build_revision: None,
        })
        .await
        .expect_err("invalid release");
    assert!(matches!(
        error,
        StoreError::AgentContractObservationInvariant {
            code: "INVALID_RELEASE_ID"
        }
    ));

    let started = store
        .create_graph_edit_message_with_observation(NewAgentContractObservation {
            user_message: user_message(&workspace_id, "safe"),
            contract_mode: AgentContractMode::Intent,
            release_id: None,
            build_revision: None,
        })
        .await
        .expect("started");
    let error = store
        .finalize_agent_contract_observation(CompleteAgentContractObservation {
            observation_id: &started.observation.id,
            workspace_id: &workspace_id,
            contract_mode: AgentContractMode::Intent,
            outcome: AgentContractOutcome::Error,
            reason_code: "token=secret",
            session_id: None,
        })
        .await
        .expect_err("free-form reason");
    assert!(matches!(
        error,
        StoreError::AgentContractObservationInvariant {
            code: "INVALID_TERMINAL_COMPLETION"
        }
    ));
}

#[tokio::test]
async fn proposal_success_and_observation_completion_share_one_transaction() {
    let (store, workspace_id) = fixture().await;
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace_id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: "graphs/base.json",
            graph_hash: "sha256:base",
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("base");
    let started = store
        .create_graph_edit_message_with_observation(NewAgentContractObservation {
            user_message: user_message(&workspace_id, "edit graph"),
            contract_mode: AgentContractMode::Intent,
            release_id: Some("v0.2.0"),
            build_revision: Some("abc123"),
        })
        .await
        .expect("started");

    let result = store
        .auto_apply_proposal_version_with_observation(
            AutoApplyProposalVersionRecord {
                proposal: NewProposal {
                    workspace_id: &workspace_id,
                    base_version_id: &base.id,
                    kind: "modify",
                    title: "Observed edit",
                    summary: "Apply it",
                    ops_path: "proposals/observed/ops.json",
                    preview_graph_path: Some("proposals/observed/preview.json"),
                    message_id: None,
                },
                version: NewVersion {
                    workspace_id: &workspace_id,
                    label: "Agent edit",
                    source: VersionSource::Proposal,
                    graph_path: "proposals/observed/applied.json",
                    graph_hash: "sha256:observed",
                    parent_id: Some(&base.id),
                    semantics_json: None,
                },
                message_text: "Applied",
            },
            CompleteAgentContractObservation {
                observation_id: &started.observation.id,
                workspace_id: &workspace_id,
                contract_mode: AgentContractMode::Intent,
                outcome: AgentContractOutcome::Success,
                reason_code: "INTENT_COMPILED",
                session_id: Some("agent_success"),
            },
        )
        .await
        .expect("observed apply");
    let observation = store
        .agent_contract_observation(&started.observation.id)
        .await
        .expect("observation");
    assert_eq!(observation.outcome, "success");
    assert_eq!(observation.reason_code.as_deref(), Some("INTENT_COMPILED"));
    assert_eq!(
        store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(result.version.id.as_str())
    );

    let conflicting = store
        .create_graph_edit_message_with_observation(NewAgentContractObservation {
            user_message: user_message(&workspace_id, "conflicting edit"),
            contract_mode: AgentContractMode::Intent,
            release_id: None,
            build_revision: None,
        })
        .await
        .expect("started");
    store
        .finalize_agent_contract_observation(CompleteAgentContractObservation {
            observation_id: &conflicting.observation.id,
            workspace_id: &workspace_id,
            contract_mode: AgentContractMode::Intent,
            outcome: AgentContractOutcome::Error,
            reason_code: "AGENT_RUNTIME_ERROR",
            session_id: None,
        })
        .await
        .expect("terminal error");
    let versions_before = store
        .versions_for_workspace(&workspace_id)
        .await
        .expect("versions")
        .len();
    let error = store
        .auto_apply_proposal_version_with_observation(
            AutoApplyProposalVersionRecord {
                proposal: NewProposal {
                    workspace_id: &workspace_id,
                    base_version_id: &result.version.id,
                    kind: "modify",
                    title: "Must roll back",
                    summary: "Must roll back",
                    ops_path: "proposals/rollback/ops.json",
                    preview_graph_path: None,
                    message_id: None,
                },
                version: NewVersion {
                    workspace_id: &workspace_id,
                    label: "Must roll back",
                    source: VersionSource::Proposal,
                    graph_path: "proposals/rollback/applied.json",
                    graph_hash: "sha256:rollback",
                    parent_id: Some(&result.version.id),
                    semantics_json: None,
                },
                message_text: "Must roll back",
            },
            CompleteAgentContractObservation {
                observation_id: &conflicting.observation.id,
                workspace_id: &workspace_id,
                contract_mode: AgentContractMode::Intent,
                outcome: AgentContractOutcome::Success,
                reason_code: "INTENT_COMPILED",
                session_id: Some("agent_conflict"),
            },
        )
        .await
        .expect_err("terminal observation conflict");
    assert!(matches!(
        error,
        StoreError::AgentContractObservationInvariant {
            code: "TERMINAL_COMPLETION_CONFLICT"
        }
    ));
    assert_eq!(
        store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions")
            .len(),
        versions_before,
        "observation conflict must roll back proposal and version"
    );
}

#[tokio::test]
async fn evidence_aggregate_filters_release_and_exposes_unattributed_inflight_rows() {
    let (store, workspace_id) = fixture().await;
    for (index, mode, outcome, reason, attributed) in [
        (
            1,
            AgentContractMode::Intent,
            Some(AgentContractOutcome::Success),
            "INTENT_COMPILED",
            true,
        ),
        (
            2,
            AgentContractMode::Intent,
            Some(AgentContractOutcome::Clarify),
            "REQUIRED_INPUT_MISSING",
            true,
        ),
        (
            3,
            AgentContractMode::Legacy,
            Some(AgentContractOutcome::Error),
            "AGENT_RUNTIME_ERROR",
            false,
        ),
        (4, AgentContractMode::Intent, None, "", false),
    ] {
        let text = format!("turn {index}");
        let started = store
            .create_graph_edit_message_with_observation(NewAgentContractObservation {
                user_message: user_message(&workspace_id, &text),
                contract_mode: mode,
                release_id: attributed.then_some("v0.2.0"),
                build_revision: attributed.then_some("abc123"),
            })
            .await
            .expect("started");
        if let Some(outcome) = outcome {
            store
                .finalize_agent_contract_observation(CompleteAgentContractObservation {
                    observation_id: &started.observation.id,
                    workspace_id: &workspace_id,
                    contract_mode: mode,
                    outcome,
                    reason_code: reason,
                    session_id: Some("agent"),
                })
                .await
                .expect("complete");
        }
    }

    let evidence = store
        .agent_contract_evidence(AgentContractEvidenceFilter {
            since: "2000-01-01 00:00:00",
            until: "2100-01-01 00:00:00",
            release_id: Some("v0.2.0"),
            build_revision: Some("abc123"),
        })
        .await
        .expect("evidence");
    assert_eq!(evidence.unattributed, 2);
    assert_eq!(evidence.in_flight, 1);
    assert_eq!(
        evidence
            .groups
            .iter()
            .map(|group| (
                group.contract_mode.as_str(),
                group.outcome.as_str(),
                group.reason_code.as_deref(),
                group.count,
            ))
            .collect::<Vec<_>>(),
        vec![
            ("intent", "clarify", Some("REQUIRED_INPUT_MISSING"), 1),
            ("intent", "success", Some("INTENT_COMPILED"), 1),
        ]
    );

    let error = store
        .agent_contract_evidence(AgentContractEvidenceFilter {
            since: "2100-01-01 00:00:00",
            until: "2000-01-01 00:00:00",
            release_id: None,
            build_revision: None,
        })
        .await
        .expect_err("invalid window");
    assert!(matches!(
        error,
        StoreError::AgentContractObservationInvariant {
            code: "INVALID_EVIDENCE_WINDOW"
        }
    ));
}
