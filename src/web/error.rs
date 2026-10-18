use crate::bucket::{NoBucket, NotFound};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// A handler error. The response body carries the full error chain, except
/// for a 500, whose body carries only the outermost context.
pub struct ApiError {
    pub status: StatusCode,
    pub error: anyhow::Error,
}

impl ApiError {
    pub fn new(status: StatusCode, error: impl Into<anyhow::Error>) -> Self {
        Self {
            status,
            error: error.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, anyhow::Error::msg(message.into()))
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, anyhow::Error::msg(message.into()))
    }

    pub fn object_store(error: anyhow::Error) -> Self {
        let status = if error.is::<NoBucket>() {
            StatusCode::SERVICE_UNAVAILABLE
        } else if error.is::<NotFound>() {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_GATEWAY
        };
        Self::new(status, error)
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let Self { status, error } = self;
        if status.is_server_error() {
            tracing::error!("{status}: {error:#}");
        } else {
            tracing::warn!("{status}: {error:#}");
        }
        let body = if status == StatusCode::INTERNAL_SERVER_ERROR {
            error.to_string()
        } else {
            format!("{error:#}")
        };
        (status, body).into_response()
    }
}

pub trait WithStatus<T> {
    fn status(self, status: StatusCode) -> Result<T, ApiError>;
}

impl<T, E: Into<anyhow::Error>> WithStatus<T> for Result<T, E> {
    fn status(self, status: StatusCode) -> Result<T, ApiError> {
        self.map_err(|e| ApiError::new(status, e))
    }
}
