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
    ArtifactPersistence(String),
    InvalidConfiguration(String),
    RunFixGuard {
        code: &'static str,
    },
    InvalidRunStatus {
        run_id: String,
        expected: &'static str,
        actual: String,
    },
    InvalidSweepPlan(String),
    ResolutionFailed {
        node_id: String,
        code: String,
        message: String,
    },
    MixedCostCurrency {
        expected: String,
        actual: String,
    },
    Interrupted(String),
    NoExecutableSteps,
    MissingInput {
        node_id: String,
        port: String,
    },
    MissingParam {
        node_id: String,
        param: String,
    },
    RunNotActive(String),
    RunClaimContention(String),
    WorkspaceBusy {
        workspace_id: String,
        active_run_id: String,
    },
    UnsupportedBuiltin(String),
    TaskJoin(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Graph(err) => write!(f, "{err}"),
            Self::Provider(err) => write!(f, "{err}"),
            Self::Store(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::ArtifactPersistence(message) => {
                write!(f, "artifact persistence failed: {message}")
            }
            Self::InvalidConfiguration(message) => write!(f, "invalid configuration: {message}"),
            Self::RunFixGuard { code } => write!(f, "run fix rejected: {code}"),
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
            Self::ResolutionFailed {
                node_id,
                code,
                message,
            } => write!(
                f,
                "implementation resolution failed for node `{node_id}` [{code}]: {message}"
            ),
            Self::MixedCostCurrency { expected, actual } => {
                write!(
                    f,
                    "mixed cost currencies are unsupported: `{expected}` and `{actual}`"
                )
            }
            Self::Interrupted(run_id) => write!(f, "run was interrupted: {run_id}"),
            Self::NoExecutableSteps => write!(f, "workflow has no executable steps"),
            Self::MissingInput { node_id, port } => {
                write!(f, "missing resolved input `{port}` for node `{node_id}`")
            }
            Self::MissingParam { node_id, param } => {
                write!(f, "missing builtin param `{param}` for node `{node_id}`")
            }
            Self::RunNotActive(run_id) => write!(f, "run is not active: {run_id}"),
            Self::RunClaimContention(run_id) => write!(
                f,
                "run `{run_id}` lost a concurrent claim for its workspace; retry the confirmation"
            ),
            Self::WorkspaceBusy {
                workspace_id,
                active_run_id,
            } => write!(
                f,
                "workspace `{workspace_id}` already has active run `{active_run_id}`"
            ),
            Self::UnsupportedBuiltin(node_type) => {
                write!(f, "unsupported builtin node type: {node_type}")
            }
            Self::TaskJoin(message) => write!(f, "run step task failed to join: {message}"),
        }
    }
}

impl std::error::Error for RunError {}

impl RunError {
    pub fn public_message(&self) -> String {
        match self {
            Self::Provider(_) => "provider execution failed".to_owned(),
            Self::Store(_) => "run persistence failed".to_owned(),
            Self::Json(_) => "run data could not be decoded".to_owned(),
            Self::ArtifactPersistence(_) => "artifact materialization failed".to_owned(),
            Self::TaskJoin(_) => "run step worker stopped unexpectedly".to_owned(),
            _ => self.to_string(),
        }
    }
}

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
