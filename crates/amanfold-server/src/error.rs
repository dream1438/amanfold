//! API errors. Messages are generic on purpose: they never say whether an
//! email address has an account, and never echo request contents.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// An error returned to the client.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The request is malformed or breaks a rule.
    #[error("bad request: {0}")]
    BadRequest(&'static str),
    /// Missing, expired or revoked session token.
    #[error("unauthorized")]
    Unauthorized,
    /// Login, email code or proof failed. Deliberately unspecific.
    #[error("verification failed")]
    VerificationFailed,
    /// Authenticated, but not allowed to do this.
    #[error("forbidden: {0}")]
    Forbidden(&'static str),
    /// No such resource (or not visible to this account).
    #[error("not found")]
    NotFound,
    /// Someone else wrote first: sync and retry. Carries the current sequence number.
    #[error("conflict")]
    Conflict {
        /// The vault's current manifest sequence number.
        seq: u64,
    },
    /// The thing being created already exists.
    #[error("already exists: {0}")]
    Exists(&'static str),
    /// Too many attempts; try later.
    #[error("too many requests")]
    RateLimited,
    /// Over a size limit or storage quota.
    #[error("too large: {0}")]
    TooLarge(&'static str),
    /// Server-side failure. Details are logged, not returned.
    #[error("internal error")]
    Internal(String),
}

impl ApiError {
    fn code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::VerificationFailed => (StatusCode::UNAUTHORIZED, "verification_failed"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Conflict { .. } => (StatusCode::CONFLICT, "conflict"),
            Self::Exists(_) => (StatusCode::CONFLICT, "exists"),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::TooLarge(_) => (StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.code();
        let body = match &self {
            Self::Internal(detail) => {
                tracing::error!(%detail, "internal error");
                json!({ "error": code })
            }
            Self::Conflict { seq } => json!({ "error": code, "seq": seq }),
            Self::BadRequest(m) | Self::Forbidden(m) | Self::Exists(m) | Self::TooLarge(m) => {
                json!({ "error": code, "message": m })
            }
            _ => json!({ "error": code }),
        };
        (status, Json(body)).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Internal(format!("database: {e}"))
    }
}

impl From<amanfold_core::Error> for ApiError {
    fn from(e: amanfold_core::Error) -> Self {
        use amanfold_core::Error as E;
        match e {
            E::Decrypt | E::BadSignature => Self::VerificationFailed,
            E::Malformed(m) | E::InvalidArgument(m) | E::WeakKdfParams(m) | E::InvalidCode(m) => {
                Self::BadRequest(m)
            }
            E::InvalidPublicKey => Self::BadRequest("invalid public key"),
            E::Merge(m) | E::Rollback(m) => Self::BadRequest(m),
        }
    }
}

/// Result type of handlers.
pub type ApiResult<T> = Result<T, ApiError>;
