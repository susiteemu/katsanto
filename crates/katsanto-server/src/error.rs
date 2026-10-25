use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use katsanto_db::DbError;
use katsanto_yt::YtError;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    /// Not logged in, or invalid credentials.
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    /// The action needs a recent re-authentication.
    #[error("please confirm it's you")]
    ReauthRequired,
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error(transparent)]
    Youtube(#[from] YtError),
    #[error("upstream error: {0}")]
    Upstream(String),
    #[error("internal error")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

/// Error body of every failed API request.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    /// Machine-readable code, e.g. `unauthorized`, `reauth_required`.
    pub error: &'static str,
    pub message: String,
}

impl From<DbError> for AppError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::Conflict(what) => AppError::Conflict(format!("{what} already exists")),
            e => AppError::Internal(e.to_string()),
        }
    }
}

impl AppError {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        AppError::Internal(e.to_string())
    }

    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            AppError::BadRequest(_) | AppError::Youtube(YtError::BadRequest(_)) => {
                (StatusCode::BAD_REQUEST, "bad_request")
            }
            AppError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::ReauthRequired => (StatusCode::FORBIDDEN, "reauth_required"),
            AppError::NotFound(_) | AppError::Youtube(YtError::NotFound(_)) => {
                (StatusCode::NOT_FOUND, "not_found")
            }
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::Youtube(YtError::Unavailable(_)) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "unavailable")
            }
            AppError::Youtube(YtError::Blocked(_)) => {
                (StatusCode::SERVICE_UNAVAILABLE, "blocked_by_youtube")
            }
            AppError::Youtube(YtError::Other(_)) | AppError::Upstream(_) => {
                (StatusCode::BAD_GATEWAY, "upstream")
            }
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        if let AppError::Internal(detail) = &self {
            tracing::error!(%detail, "internal error");
        } else if status.is_server_error() {
            tracing::warn!(error = %self, "request failed");
        }
        let body = ErrorBody {
            error: code,
            message: self.to_string(),
        };
        (status, Json(body)).into_response()
    }
}
