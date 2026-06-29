use axum::{Json, http::StatusCode, response::IntoResponse};
use helixflow_agent::AgentError;
use helixflow_run::RunError;
use helixflow_store::StoreError;
use serde_json::json;

#[derive(Debug)]
pub(crate) struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
}

impl ApiError {
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
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
        }
    }

    pub(crate) fn agent(err: AgentError) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: err.to_string(),
        }
    }

    pub(crate) fn run(err: RunError) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: err.to_string(),
        }
    }

    pub(crate) fn io(context: impl Into<String>, err: std::io::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("{}: {err}", context.into()),
        }
    }

    pub(crate) fn server_error(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(json!({
                "error": self.message
            })),
        )
            .into_response()
    }
}
