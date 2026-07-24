use std::collections::BTreeMap;

use helixflow_graph::{ExecutionPlan, ExecutionStep};
use serde_json::json;

use super::*;

#[test]
fn dag_counts_shared_upstream_once_and_releases_join_node_last() {
    let plan = ExecutionPlan {
        schema_version: 1,
        version_id: "ver".to_owned(),
        steps: vec![
            step("a", []),
            step("b", [("text", ["a", "text"])]),
            step("c", [("text", ["a", "text"])]),
            step(
                "d",
                [
                    ("left", ["b", "prompt"]),
                    ("right", ["c", "prompt"]),
                    ("right_again", ["c", "alt"]),
                ],
            ),
        ],
    };

    let dag = StepDag::from_plan(&plan).expect("dag");

    assert_eq!(dag.upstream_counts, vec![0, 1, 1, 2]);
    assert_eq!(dag.initial_ready(), VecDeque::from([0]));
    assert_eq!(dag.downstream[0], vec![1, 2]);
    assert_eq!(dag.downstream[1], vec![3]);
    assert_eq!(dag.downstream[2], vec![3]);
}

fn step<const N: usize>(node_id: &str, inputs: [(&str, [&str; 2]); N]) -> ExecutionStep {
    ExecutionStep {
        node_id: node_id.to_owned(),
        node_type: "test.node".to_owned(),
        provider: None,
        capability: None,
        inputs: inputs
            .into_iter()
            .map(|(port, source)| {
                (
                    port.to_owned(),
                    [source[0].to_owned(), source[1].to_owned()],
                )
            })
            .collect::<BTreeMap<_, _>>(),
        params: json!({}),
    }
}
