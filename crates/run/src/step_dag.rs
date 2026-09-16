use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
use std::collections::VecDeque;

use helixflow_graph::ExecutionPlan;

use super::{RunError, RunResult};

#[derive(Debug)]
pub(super) struct StepDag {
    pub(super) upstream_counts: Vec<usize>,
    pub(super) downstream: Vec<Vec<usize>>,
}

impl StepDag {
    pub(super) fn from_plan(plan: &ExecutionPlan) -> RunResult<Self> {
        let mut by_node = BTreeMap::new();
        for (index, step) in plan.steps.iter().enumerate() {
            by_node.insert(step.node_id.as_str(), index);
        }

        let mut upstream = vec![BTreeSet::new(); plan.steps.len()];
        let mut downstream = vec![BTreeSet::new(); plan.steps.len()];
        for (index, step) in plan.steps.iter().enumerate() {
            for sources in step.inputs.values() {
                for source in sources {
                    let Some(&source_index) = by_node.get(source[0].as_str()) else {
                        return Err(RunError::MissingInput {
                            node_id: source[0].clone(),
                            port: source[1].clone(),
                        });
                    };
                    upstream[index].insert(source_index);
                    downstream[source_index].insert(index);
                }
            }
        }

        Ok(Self {
            upstream_counts: upstream.iter().map(BTreeSet::len).collect(),
            downstream: downstream
                .into_iter()
                .map(|items| items.into_iter().collect())
                .collect(),
        })
    }

    #[cfg(test)]
    pub(super) fn initial_ready(&self) -> VecDeque<usize> {
        self.upstream_counts
            .iter()
            .enumerate()
            .filter_map(|(index, count)| (*count == 0).then_some(index))
            .collect()
    }
}
