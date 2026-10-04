use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("authentication required")]
    Unauthorized,
    #[error("insufficient permissions")]
    Forbidden,
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Unprocessable(String),
    #[error("{0}")]
    Unavailable(String),
    #[error("Book downloading isn't set up yet.")]
    IndexerNotConfigured,
    #[error("too many requests")]
    RateLimited,
    #[error("cryptographic operation failed")]
    Crypto,
    #[error(transparent)]
    Library(#[from] bokhylle_library::LibraryError),
    #[error(transparent)]
    Internal(#[from] sqlx::Error),
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ErrorBody {
    code: &'static str,
    message: String,
    details: Option<Value>,
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Unprocessable(_) => "unprocessable_entity",
            Self::Unavailable(_) => "service_unavailable",
            Self::IndexerNotConfigured => "indexer_not_configured",
            Self::RateLimited => "rate_limited",
            Self::Crypto => "internal_error",
            Self::Library(_) => "internal_error",
            Self::Internal(_) => "internal_error",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Unprocessable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unavailable(_) | Self::IndexerNotConfigured => StatusCode::SERVICE_UNAVAILABLE,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Crypto => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Library(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn message(&self) -> String {
        match self {
            Self::Internal(error) => {
                tracing::error!(error = %error, "internal error");
                "internal error".to_string()
            }
            Self::Library(error) => {
                tracing::error!(error = %error, "library error");
                "internal error".to_string()
            }
            other => other.to_string(),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        AppError::Library(bokhylle_library::LibraryError::Io(error))
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = ErrorBody {
            code: self.code(),
            message: self.message(),
            details: None,
        };
        (status, Json(body)).into_response()
    }
}

impl aide::operation::OperationOutput for AppError {
    type Inner = Self;

    fn inferred_responses(
        ctx: &mut aide::generate::GenContext,
        operation: &mut aide::openapi::Operation,
    ) -> Vec<(Option<u16>, aide::openapi::Response)> {
        use aide::operation::OperationOutput;

        <Json<ErrorBody> as OperationOutput>::operation_response(ctx, operation)
            .map(|mut response| {
                response.description =
                    "API error. The HTTP status and `code` identify the failure.".to_string();
                (None, response)
            })
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_variants_to_codes_and_statuses() {
        let cases: Vec<(AppError, &str, StatusCode)> = vec![
            (
                AppError::BadRequest("invalid".into()),
                "bad_request",
                StatusCode::BAD_REQUEST,
            ),
            (
                AppError::Unauthorized,
                "unauthorized",
                StatusCode::UNAUTHORIZED,
            ),
            (AppError::Forbidden, "forbidden", StatusCode::FORBIDDEN),
            (
                AppError::NotFound("missing".into()),
                "not_found",
                StatusCode::NOT_FOUND,
            ),
            (
                AppError::Conflict("duplicate".into()),
                "conflict",
                StatusCode::CONFLICT,
            ),
            (
                AppError::Unprocessable("unprocessable".into()),
                "unprocessable_entity",
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                AppError::Unavailable("down".into()),
                "service_unavailable",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                AppError::RateLimited,
                "rate_limited",
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (
                AppError::Crypto,
                "internal_error",
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];

        for (error, code, status) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(error.status(), status);
        }
    }

    #[test]
    fn internal_error_hides_sqlx_details() {
        let error = AppError::Internal(sqlx::Error::PoolClosed);
        assert_eq!(error.code(), "internal_error");
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.message(), "internal error");
    }
}
