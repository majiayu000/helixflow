use axum::{Json, http::StatusCode, response::IntoResponse};
use helixflow_agent::AgentError;
use helixflow_run::RunError;
use helixflow_store::StoreError;
use serde_json::json;

#[derive(Debug)]
pub(crate) struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
    details: Option<serde_json::Value>,
}

impl ApiError {
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn conflict_with_details(
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
            details: Some(details),
        }
    }

    pub(crate) fn bad_request_with_details(
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            details: Some(details),
        }
    }

    pub(crate) fn not_found_with_details(
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
            details: Some(details),
        }
    }

    pub(crate) fn store(err: StoreError) -> Self {
        if err.is_not_found() {
            return Self::not_found("workspace record was not found");
        }
        if matches!(
            err,
            StoreError::VersionConflict { .. } | StoreError::ProposalStateConflict { .. }
        ) {
            return Self::bad_request(err.to_string());
        }
        if matches!(err, StoreError::ProposalWorkspaceMismatch { .. }) {
            return Self::not_found(err.to_string());
        }
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
            details: None,
        }
    }

    pub(crate) fn agent(err: AgentError) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: err.to_string(),
            details: None,
        }
    }

    pub(crate) fn run(err: RunError) -> Self {
        match err {
            RunError::Graph(_)
            | RunError::NoExecutableSteps
            | RunError::MissingInput { .. }
            | RunError::MissingParam { .. }
            | RunError::UnsupportedBuiltin(_) => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message: err.to_string(),
                details: None,
            },
            RunError::InvalidRunStatus { .. }
            | RunError::RunNotActive(_)
            | RunError::WorkspaceBusy { .. }
            | RunError::Interrupted(_) => Self {
                status: StatusCode::CONFLICT,
                message: err.to_string(),
                details: None,
            },
            RunError::Store(err) => Self::store(err),
            RunError::Json(err) => Self::server_error(err.to_string()),
            RunError::InvalidConfiguration(message) => Self::server_error(message),
            RunError::TaskJoin(_) => Self::server_error(err.to_string()),
            RunError::Provider(_)
            | RunError::ArtifactPersistence(_)
            | RunError::InvalidSweepPlan(_)
            | RunError::MixedCostCurrency { .. } => Self {
                status: StatusCode::BAD_GATEWAY,
                message: err.to_string(),
                details: None,
            },
        }
    }

    pub(crate) fn io(context: impl Into<String>, err: std::io::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("{}: {err}", context.into()),
            details: None,
        }
    }

    pub(crate) fn server_error(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
            details: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let mut body = json!({
            "error": self.message
        });
        if let Some(details) = self.details
            && let (Some(body), Some(details)) = (body.as_object_mut(), details.as_object())
        {
            body.extend(details.clone());
        }
        (self.status, Json(body)).into_response()
    }
}
