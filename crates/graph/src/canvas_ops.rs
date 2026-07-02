use serde::Deserialize;
use serde_json::{Map, Value};

use crate::{
    CanvasComment, CanvasDocument, CanvasEdge, CanvasError, CanvasNode, CanvasOpEnvelope,
    CanvasOpKind, CanvasPoint, CanvasResult, CanvasRuntimeStatus, CanvasSize,
};

impl CanvasDocument {
    pub fn apply_op(&mut self, op: &CanvasOpEnvelope) -> CanvasResult<()> {
        self.validate_op_envelope(op)?;

        let mut next = self.clone();
        next.apply_op_mutation(op)?;
        next.seq = op.seq;
        next.updated_at = op.created_at.clone();
        *self = next;
        Ok(())
    }

    pub fn replay_ops(&mut self, ops: &[CanvasOpEnvelope]) -> CanvasResult<()> {
        for op in ops {
            self.apply_op(op)?;
        }
        Ok(())
    }

    fn validate_op_envelope(&self, op: &CanvasOpEnvelope) -> CanvasResult<()> {
        if op.canvas_id != self.canvas_id {
            return Err(CanvasError::CanvasIdMismatch {
                expected: self.canvas_id.clone(),
                actual: op.canvas_id.clone(),
            });
        }
        let expected_seq = self.seq + 1;
        if op.seq != expected_seq {
            return Err(CanvasError::UnexpectedSeq {
                expected: expected_seq,
                actual: op.seq,
            });
        }
        if op.base_seq > self.seq {
            return Err(CanvasError::FutureBaseSeq {
                base_seq: op.base_seq,
                current_seq: self.seq,
            });
        }
        Ok(())
    }

    fn apply_op_mutation(&mut self, op: &CanvasOpEnvelope) -> CanvasResult<()> {
        match op.kind {
            CanvasOpKind::NodeAdd => {
                let payload: NodeAddPayload = payload(op)?;
                if self.nodes.contains_key(&payload.node.id) {
                    return Err(CanvasError::DuplicateNode(payload.node.id));
                }
                self.nodes.insert(payload.node.id.clone(), payload.node);
            }
            CanvasOpKind::NodePatch => {
                let payload: NodePatchPayload = payload(op)?;
                let node = self
                    .nodes
                    .get_mut(&payload.node_id)
                    .ok_or_else(|| CanvasError::MissingNode(payload.node_id.clone()))?;
                apply_node_patch(node, payload.patch, payload.prev, &op.created_at)?;
            }
            CanvasOpKind::NodeMove => {
                let payload: NodeMovePayload = payload(op)?;
                let node = self
                    .nodes
                    .get_mut(&payload.node_id)
                    .ok_or_else(|| CanvasError::MissingNode(payload.node_id.clone()))?;
                node.position = payload.position;
                node.updated_at = op.created_at.clone();
            }
            CanvasOpKind::NodeResize => {
                let payload: NodeResizePayload = payload(op)?;
                let node = self
                    .nodes
                    .get_mut(&payload.node_id)
                    .ok_or_else(|| CanvasError::MissingNode(payload.node_id.clone()))?;
                node.size = Some(payload.size);
                node.updated_at = op.created_at.clone();
            }
            CanvasOpKind::NodeDelete => {
                let payload: NodeDeletePayload = payload(op)?;
                self.nodes
                    .remove(&payload.node_id)
                    .ok_or_else(|| CanvasError::MissingNode(payload.node_id.clone()))?;
                self.edges.retain(|_, edge| {
                    edge.from.node_id != payload.node_id && edge.to.node_id != payload.node_id
                });
                self.comments
                    .retain(|_, comment| comment.anchor.node_id.as_ref() != Some(&payload.node_id));
            }
            CanvasOpKind::EdgeAdd => {
                let payload: EdgeAddPayload = payload(op)?;
                ensure_edge_endpoints(self, &payload.edge)?;
                if self.edges.contains_key(&payload.edge.id) {
                    return Err(CanvasError::DuplicateEdge(payload.edge.id));
                }
                self.edges.insert(payload.edge.id.clone(), payload.edge);
            }
            CanvasOpKind::EdgeDelete => {
                let payload: EdgeDeletePayload = payload(op)?;
                self.edges
                    .remove(&payload.edge_id)
                    .ok_or_else(|| CanvasError::MissingEdge(payload.edge_id.clone()))?;
            }
            CanvasOpKind::CommentAdd => {
                let payload: CommentAddPayload = payload(op)?;
                if self.comments.contains_key(&payload.comment.id) {
                    return Err(CanvasError::DuplicateComment(payload.comment.id));
                }
                self.comments
                    .insert(payload.comment.id.clone(), payload.comment);
            }
            CanvasOpKind::CommentPatch => {
                let payload: CommentPatchPayload = payload(op)?;
                let comment = self
                    .comments
                    .get_mut(&payload.comment_id)
                    .ok_or_else(|| CanvasError::MissingComment(payload.comment_id.clone()))?;
                apply_comment_patch(comment, payload.patch, payload.prev, &op.created_at)?;
            }
            CanvasOpKind::CommentDelete => {
                let payload: CommentDeletePayload = payload(op)?;
                self.comments
                    .remove(&payload.comment_id)
                    .ok_or_else(|| CanvasError::MissingComment(payload.comment_id.clone()))?;
            }
            CanvasOpKind::RunRequest => {
                let RunRequestPayload { label: _label } = payload(op)?;
            }
            CanvasOpKind::ArtifactAttach => {
                let payload: ArtifactAttachPayload = payload(op)?;
                let node = self
                    .nodes
                    .get_mut(&payload.node_id)
                    .ok_or_else(|| CanvasError::MissingNode(payload.node_id.clone()))?;
                if !node.runtime.artifact_ids.contains(&payload.artifact_id) {
                    node.runtime.artifact_ids.push(payload.artifact_id);
                }
                if let Some(run_id) = payload.run_id {
                    node.runtime.run_id = Some(run_id);
                }
                if let Some(run_step_id) = payload.run_step_id {
                    node.runtime.run_step_id = Some(run_step_id);
                }
                node.runtime.status = payload.status.unwrap_or(CanvasRuntimeStatus::Succeeded);
                node.runtime.error = None;
                node.updated_at = op.created_at.clone();
            }
            CanvasOpKind::ProposalApply => {
                let payload: ProposalApplyPayload = payload(op)?;
                if let Some(base_graph_version_id) = payload.base_graph_version_id {
                    self.base_graph_version_id = Some(base_graph_version_id);
                }
                if let Some(document_version_id) = payload.document_version_id {
                    self.document_version_id = Some(document_version_id);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeAddPayload {
    node: CanvasNode,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodePatchPayload {
    node_id: String,
    patch: Value,
    #[serde(default)]
    prev: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeMovePayload {
    node_id: String,
    position: CanvasPoint,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeResizePayload {
    node_id: String,
    size: CanvasSize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeDeletePayload {
    node_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeAddPayload {
    edge: CanvasEdge,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeDeletePayload {
    edge_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentAddPayload {
    comment: CanvasComment,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentPatchPayload {
    comment_id: String,
    patch: Value,
    #[serde(default)]
    prev: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentDeletePayload {
    comment_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunRequestPayload {
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactAttachPayload {
    node_id: String,
    artifact_id: String,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    run_step_id: Option<String>,
    #[serde(default)]
    status: Option<CanvasRuntimeStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalApplyPayload {
    #[serde(default)]
    base_graph_version_id: Option<String>,
    #[serde(default)]
    document_version_id: Option<String>,
}

fn payload<T: for<'de> Deserialize<'de>>(op: &CanvasOpEnvelope) -> CanvasResult<T> {
    serde_json::from_value(op.payload.clone()).map_err(|err| CanvasError::InvalidPayload {
        kind: op.kind,
        message: err.to_string(),
    })
}

fn ensure_edge_endpoints(document: &CanvasDocument, edge: &CanvasEdge) -> CanvasResult<()> {
    if !document.nodes.contains_key(&edge.from.node_id) {
        return Err(CanvasError::MissingEndpoint {
            edge_id: edge.id.clone(),
            node_id: edge.from.node_id.clone(),
        });
    }
    if !document.nodes.contains_key(&edge.to.node_id) {
        return Err(CanvasError::MissingEndpoint {
            edge_id: edge.id.clone(),
            node_id: edge.to.node_id.clone(),
        });
    }
    Ok(())
}

fn apply_node_patch(
    node: &mut CanvasNode,
    patch: Value,
    prev: Option<Value>,
    now: &str,
) -> CanvasResult<()> {
    validate_patch_fields(
        "node",
        &node.id,
        &patch,
        &[
            "title",
            "node_type",
            "params",
            "content",
            "media",
            "runtime",
            "ui",
        ],
    )?;
    let current = serde_json::to_value(&*node).expect("canvas node serializes");
    if let Some(prev) = &prev {
        ensure_prev_matches("node", &node.id, &current, prev, "")?;
    }
    let mut next = current;
    merge_patch(&mut next, patch);
    let mut next_node: CanvasNode =
        serde_json::from_value(next).map_err(|err| CanvasError::InvalidPayload {
            kind: CanvasOpKind::NodePatch,
            message: err.to_string(),
        })?;
    next_node.updated_at = now.to_owned();
    *node = next_node;
    Ok(())
}

fn apply_comment_patch(
    comment: &mut CanvasComment,
    patch: Value,
    prev: Option<Value>,
    now: &str,
) -> CanvasResult<()> {
    validate_patch_fields(
        "comment",
        &comment.id,
        &patch,
        &["anchor", "body", "resolved"],
    )?;
    let current = serde_json::to_value(&*comment).expect("canvas comment serializes");
    if let Some(prev) = &prev {
        ensure_prev_matches("comment", &comment.id, &current, prev, "")?;
    }
    let mut next = current;
    merge_patch(&mut next, patch);
    let mut next_comment: CanvasComment =
        serde_json::from_value(next).map_err(|err| CanvasError::InvalidPayload {
            kind: CanvasOpKind::CommentPatch,
            message: err.to_string(),
        })?;
    next_comment.updated_at = now.to_owned();
    *comment = next_comment;
    Ok(())
}

fn validate_patch_fields(
    entity: &'static str,
    id: &str,
    patch: &Value,
    allowed_fields: &[&str],
) -> CanvasResult<()> {
    let Some(fields) = patch.as_object() else {
        return Err(CanvasError::PatchNotObject {
            entity,
            id: id.to_owned(),
        });
    };
    for field in fields.keys() {
        if !allowed_fields.contains(&field.as_str()) {
            return Err(CanvasError::UnsupportedPatchField {
                entity,
                field: field.clone(),
            });
        }
    }
    Ok(())
}

fn ensure_prev_matches(
    entity: &'static str,
    id: &str,
    current: &Value,
    expected: &Value,
    path: &str,
) -> CanvasResult<()> {
    let Some(expected_object) = expected.as_object() else {
        if current == expected {
            return Ok(());
        }
        return Err(CanvasError::PatchConflict {
            entity,
            id: id.to_owned(),
            field: path.to_owned(),
        });
    };

    let Some(current_object) = current.as_object() else {
        return Err(CanvasError::PatchConflict {
            entity,
            id: id.to_owned(),
            field: path.to_owned(),
        });
    };

    for (key, expected_value) in expected_object {
        let field = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        let Some(current_value) = current_object.get(key) else {
            return Err(CanvasError::PatchConflict {
                entity,
                id: id.to_owned(),
                field,
            });
        };
        ensure_prev_matches(entity, id, current_value, expected_value, &field)?;
    }
    Ok(())
}

fn merge_patch(target: &mut Value, patch: Value) {
    let Value::Object(patch_fields) = patch else {
        *target = patch;
        return;
    };

    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    let target_fields = target.as_object_mut().expect("target is object");
    for (key, value) in patch_fields {
        if value.is_null() {
            target_fields.remove(&key);
        } else if value.is_object() {
            let target_value = target_fields
                .entry(key)
                .or_insert_with(|| Value::Object(Map::new()));
            merge_patch(target_value, value);
        } else {
            target_fields.insert(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        CanvasActor, CanvasActorKind, CanvasComment, CanvasEdge, CanvasEdgeKind, CanvasEndpoint,
        CanvasError, CanvasNode, CanvasOpEnvelope, CanvasOpKind, CanvasRuntimeStatus, CanvasSize,
        canvas_tests::{sample_comment, sample_document, sample_text_node},
    };

    const NOW_2: &str = "2026-07-01T00:00:01Z";

    fn op(kind: CanvasOpKind, seq: u64, payload: serde_json::Value) -> CanvasOpEnvelope {
        CanvasOpEnvelope {
            op_id: format!("op_{seq}"),
            canvas_id: "canvas_1".to_owned(),
            seq,
            base_seq: seq - 1,
            actor: CanvasActor {
                id: "user_1".to_owned(),
                kind: CanvasActorKind::User,
            },
            kind,
            payload,
            idempotency_key: format!("client_{seq}"),
            created_at: NOW_2.to_owned(),
        }
    }

    #[test]
    fn applies_node_add_move_and_artifact_attach_ops() {
        let mut document = sample_document();
        let note: CanvasNode = sample_text_node("note");

        document
            .apply_op(&op(
                CanvasOpKind::NodeAdd,
                8,
                json!({
                    "node": note
                }),
            ))
            .expect("add node");
        document
            .apply_op(&op(
                CanvasOpKind::NodeMove,
                9,
                json!({
                    "node_id": "note",
                    "position": { "x": 50.0, "y": 60.0 }
                }),
            ))
            .expect("move node");
        document
            .apply_op(&op(
                CanvasOpKind::ArtifactAttach,
                10,
                json!({
                    "node_id": "video",
                    "artifact_id": "artifact_1",
                    "run_id": "run_1",
                    "run_step_id": "step_1"
                }),
            ))
            .expect("attach artifact");

        assert_eq!(document.seq, 10);
        assert_eq!(document.nodes["note"].position.x, 50.0);
        assert_eq!(
            document.nodes["video"].runtime.status,
            CanvasRuntimeStatus::Succeeded
        );
        assert_eq!(
            document.nodes["video"].runtime.artifact_ids,
            vec!["artifact_1".to_owned()]
        );
    }

    #[test]
    fn rejects_non_next_op_sequence() {
        let mut document = sample_document();
        let err = document
            .apply_op(&op(CanvasOpKind::RunRequest, 10, json!({ "label": "Run" })))
            .expect_err("seq gap should fail");

        assert!(matches!(
            err,
            CanvasError::UnexpectedSeq {
                expected: 8,
                actual: 10
            }
        ));
    }

    #[test]
    fn node_patch_requires_matching_prev_values() {
        let mut document = sample_document();

        let err = document
            .apply_op(&op(
                CanvasOpKind::NodePatch,
                8,
                json!({
                    "node_id": "video",
                    "patch": { "params": { "duration_sec": 3 } },
                    "prev": { "params": { "duration_sec": 9 } }
                }),
            ))
            .expect_err("stale param should conflict");

        assert!(matches!(
            err,
            CanvasError::PatchConflict { entity: "node", id, field }
                if id == "video" && field == "params.duration_sec"
        ));
    }

    #[test]
    fn node_patch_updates_params_when_prev_matches() {
        let mut document = sample_document();

        document
            .apply_op(&op(
                CanvasOpKind::NodePatch,
                8,
                json!({
                    "node_id": "video",
                    "patch": { "params": { "duration_sec": 3 } },
                    "prev": { "params": { "duration_sec": 5 } }
                }),
            ))
            .expect("patch node");

        assert_eq!(document.nodes["video"].params["duration_sec"], 3);
        assert_eq!(document.nodes["video"].updated_at, NOW_2);
    }

    #[test]
    fn deleting_node_removes_attached_edges_and_comments() {
        let mut document = sample_document();
        let edge = CanvasEdge {
            id: "edge_1".to_owned(),
            from: CanvasEndpoint {
                node_id: "video".to_owned(),
                port: "video".to_owned(),
            },
            to: CanvasEndpoint {
                node_id: "video".to_owned(),
                port: "prompt".to_owned(),
            },
            kind: CanvasEdgeKind::Visual,
            edge_type: None,
            label: None,
            metadata: json!({}),
            created_at: NOW_2.to_owned(),
            updated_at: NOW_2.to_owned(),
        };
        let comment: CanvasComment = sample_comment("comment_1");
        document.edges.insert(edge.id.clone(), edge);
        document.comments.insert(comment.id.clone(), comment);

        document
            .apply_op(&op(
                CanvasOpKind::NodeDelete,
                8,
                json!({ "node_id": "video" }),
            ))
            .expect("delete node");

        assert!(!document.nodes.contains_key("video"));
        assert!(document.edges.is_empty());
        assert!(document.comments.is_empty());
    }

    #[test]
    fn edge_add_requires_existing_endpoints() {
        let mut document = sample_document();
        let edge = CanvasEdge {
            id: "edge_missing".to_owned(),
            from: CanvasEndpoint {
                node_id: "missing".to_owned(),
                port: "text".to_owned(),
            },
            to: CanvasEndpoint {
                node_id: "video".to_owned(),
                port: "prompt".to_owned(),
            },
            kind: CanvasEdgeKind::Data,
            edge_type: Some("text".to_owned()),
            label: None,
            metadata: json!({}),
            created_at: NOW_2.to_owned(),
            updated_at: NOW_2.to_owned(),
        };

        let err = document
            .apply_op(&op(CanvasOpKind::EdgeAdd, 8, json!({ "edge": edge })))
            .expect_err("missing endpoint should fail");

        assert!(matches!(
            err,
            CanvasError::MissingEndpoint { edge_id, node_id }
                if edge_id == "edge_missing" && node_id == "missing"
        ));
    }

    #[test]
    fn comment_patch_updates_body_and_resolution() {
        let mut document = sample_document();
        let comment = sample_comment("comment_1");
        document
            .apply_op(&op(
                CanvasOpKind::CommentAdd,
                8,
                json!({ "comment": comment }),
            ))
            .expect("add comment");
        document
            .apply_op(&op(
                CanvasOpKind::CommentPatch,
                9,
                json!({
                    "comment_id": "comment_1",
                    "patch": { "body": "Resolved", "resolved": true },
                    "prev": { "resolved": false }
                }),
            ))
            .expect("patch comment");

        assert_eq!(document.comments["comment_1"].body, "Resolved");
        assert!(document.comments["comment_1"].resolved);
    }

    #[test]
    fn node_resize_updates_size() {
        let mut document = sample_document();

        document
            .apply_op(&op(
                CanvasOpKind::NodeResize,
                8,
                json!({
                    "node_id": "video",
                    "size": { "width": 320.0, "height": 180.0 }
                }),
            ))
            .expect("resize node");

        assert_eq!(
            document.nodes["video"].size,
            Some(CanvasSize {
                width: 320.0,
                height: 180.0
            })
        );
    }
}
