use std::fmt;

use helixflow_gateway::ProviderError;
use helixflow_graph::GraphError;
use helixflow_store::StoreError;

pub type RunResult<T> = Result<T, RunError>;

#[derive(Debug)]
pub enum RunError {
    Graph(GraphError),
    Provider(ProviderError),
    Store(StoreError),
    Json(serde_json::Error),
    InvalidRunStatus {
        run_id: String,
        expected: &'static str,
        actual: String,
    },
    InvalidSweepPlan(String),
    MixedCostCurrency {
        expected: String,
        actual: String,
    },
    MissingInput {
        node_id: String,
        port: String,
    },
    MissingParam {
        node_id: String,
        param: String,
    },
    RunNotActive(String),
    UnsupportedBuiltin(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Graph(err) => write!(f, "{err}"),
            Self::Provider(err) => write!(f, "{err}"),
            Self::Store(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::InvalidRunStatus {
                run_id,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "run `{run_id}` expected status `{expected}` but found `{actual}`"
                )
            }
            Self::InvalidSweepPlan(message) => write!(f, "invalid sweep plan: {message}"),
            Self::MixedCostCurrency { expected, actual } => {
                write!(
                    f,
                    "mixed cost currencies are unsupported: `{expected}` and `{actual}`"
                )
            }
            Self::MissingInput { node_id, port } => {
                write!(f, "missing resolved input `{port}` for node `{node_id}`")
            }
            Self::MissingParam { node_id, param } => {
                write!(f, "missing builtin param `{param}` for node `{node_id}`")
            }
            Self::RunNotActive(run_id) => write!(f, "run is not active: {run_id}"),
            Self::UnsupportedBuiltin(node_type) => {
                write!(f, "unsupported builtin node type: {node_type}")
            }
        }
    }
}

impl std::error::Error for RunError {}

impl From<GraphError> for RunError {
    fn from(err: GraphError) -> Self {
        Self::Graph(err)
    }
}

impl From<ProviderError> for RunError {
    fn from(err: ProviderError) -> Self {
        Self::Provider(err)
    }
}

impl From<StoreError> for RunError {
    fn from(err: StoreError) -> Self {
        Self::Store(err)
    }
}

impl From<serde_json::Error> for RunError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}
