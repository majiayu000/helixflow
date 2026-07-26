//! Minimal deterministic diff between the current graph and a compiled
//! target (tech.md §6 step 9). Identical inputs yield identical op lists.

use helixflow_graph::{ProposalOp, WorkflowGraph};

pub(crate) fn diff(current: &WorkflowGraph, target: &WorkflowGraph) -> Vec<ProposalOp> {
    let mut ops = Vec::new();

    // Edges first so node removals never orphan a connection.
    let mut removed_edges: Vec<_> = current
        .edges
        .iter()
        .filter(|edge| !target.edges.contains(edge))
        .cloned()
        .collect();
    removed_edges.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    for edge in removed_edges {
        ops.push(ProposalOp::RemoveEdge { edge });
    }

    for (node_id, _) in current
        .nodes
        .iter()
        .filter(|(node_id, _)| !target.nodes.contains_key(*node_id))
    {
        ops.push(ProposalOp::RemoveNode {
            id: node_id.clone(),
        });
    }

    for (node_id, node) in &target.nodes {
        match current.nodes.get(node_id) {
            None => ops.push(ProposalOp::AddNode {
                id: node_id.clone(),
                node: node.clone(),
            }),
            Some(existing) if existing.node_type != node.node_type => {
                // Same id, different executable type: replace wholesale.
                ops.push(ProposalOp::RemoveNode {
                    id: node_id.clone(),
                });
                ops.push(ProposalOp::AddNode {
                    id: node_id.clone(),
                    node: node.clone(),
                });
            }
            Some(existing) => {
                if let (Some(target_params), Some(current_params)) =
                    (node.params.as_object(), existing.params.as_object())
                {
                    for (key, value) in target_params {
                        if current_params.get(key) != Some(value) {
                            ops.push(ProposalOp::SetParam {
                                id: node_id.clone(),
                                key: key.clone(),
                                prev: current_params.get(key).cloned(),
                                value: value.clone(),
                            });
                        }
                    }
                }
                // Position convergence is cosmetic; the compiler emits it so
                // fresh layouts apply, but it never carries semantics (P1).
                if existing.pos != node.pos {
                    ops.push(ProposalOp::MoveNode {
                        id: node_id.clone(),
                        pos: node.pos,
                    });
                }
                // Embedded semantics converge explicitly (GH145): a kept node
                // whose capability binding changed must not retain the stale
                // entry after apply.
                if existing.semantics != node.semantics {
                    ops.push(ProposalOp::SetSemantics {
                        id: node_id.clone(),
                        semantics: node.semantics.clone(),
                    });
                }
            }
        }
    }

    let mut added_edges: Vec<_> = target
        .edges
        .iter()
        .filter(|edge| !current.edges.contains(edge))
        .cloned()
        .collect();
    added_edges.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    for edge in added_edges {
        ops.push(ProposalOp::AddEdge { edge });
    }

    ops
}
