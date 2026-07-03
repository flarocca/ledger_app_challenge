use crate::services::auth::AuthServiceError;
use crate::services::currencies::CurrenciesServiceError;
use crate::services::feed::FeedServiceError;
use crate::services::transfers::TransfersServiceError;
use crate::services::users::UsersServiceError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("invalid request: {0}")]
    BadRequest(String),

    #[error("authentication required")]
    Unauthorized,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("{0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("unprocessable: {0}")]
    Unprocessable(String),

    #[error("internal error")]
    Internal(#[from] anyhow::Error),
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn code(&self) -> &'static str {
        match self {
            ApiError::BadRequest(_) => "BAD_REQUEST",
            ApiError::Unauthorized => "UNAUTHORIZED",
            ApiError::Forbidden(_) => "FORBIDDEN",
            ApiError::NotFound(_) => "NOT_FOUND",
            ApiError::Conflict(_) => "CONFLICT",
            ApiError::Unprocessable(_) => "UNPROCESSABLE",
            ApiError::Internal(_) => "INTERNAL",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::Unprocessable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn to_detail(&self) -> ErrorDetail {
        ErrorDetail {
            code: self.code().to_string(),
            message: match self {
                ApiError::Internal(_) => "internal error".to_string(),
                _ => self.to_string(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if matches!(self, ApiError::Internal(_)) {
            tracing::error!("internal error: {self:?}");
        }
        let status = self.status();
        let envelope = crate::response::ApiResponse::<()>::error(self.to_detail(), None, None);
        (status, axum::Json(envelope)).into_response()
    }
}

impl From<validator::ValidationErrors> for ApiError {
    fn from(err: validator::ValidationErrors) -> Self {
        ApiError::BadRequest(err.to_string())
    }
}

impl From<crate::models::MoneyError> for ApiError {
    fn from(err: crate::models::MoneyError) -> Self {
        ApiError::BadRequest(err.to_string())
    }
}

impl From<UsersServiceError> for ApiError {
    fn from(err: UsersServiceError) -> Self {
        match err {
            UsersServiceError::AccountNotFound(_) => ApiError::NotFound(err.to_string()),
            UsersServiceError::Repository(_) => ApiError::Internal(anyhow::anyhow!(err)),
        }
    }
}

impl From<AuthServiceError> for ApiError {
    fn from(err: AuthServiceError) -> Self {
        match err {
            AuthServiceError::InvalidCredentials | AuthServiceError::InvalidSession => {
                ApiError::Unauthorized
            }
            AuthServiceError::HashError(_)
            | AuthServiceError::UsersRepository(_)
            | AuthServiceError::SessionsRepository(_) => ApiError::Internal(anyhow::anyhow!(err)),
        }
    }
}

impl From<CurrenciesServiceError> for ApiError {
    fn from(err: CurrenciesServiceError) -> Self {
        match err {
            CurrenciesServiceError::UnsupportedCurrency(_) => {
                ApiError::Unprocessable(err.to_string())
            }
            CurrenciesServiceError::Repository(_) => ApiError::Internal(anyhow::anyhow!(err)),
        }
    }
}

impl From<TransfersServiceError> for ApiError {
    fn from(err: TransfersServiceError) -> Self {
        use TransfersServiceError as E;
        match err {
            E::SelfTransfer
            | E::CurrencyMismatch
            | E::InsufficientFunds
            | E::DuplicateRecipient(_)
            | E::NoRecipients => ApiError::Unprocessable(err.to_string()),
            E::RecipientNotFound(_) => ApiError::NotFound(err.to_string()),
            E::SenderNotFound(_) => ApiError::Unauthorized,
            E::SystemAccountNotAllowed => ApiError::Forbidden(err.to_string()),
            E::InvariantViolation(_) | E::Ledger(_) => ApiError::Internal(anyhow::anyhow!(err)),
            E::UsersRepository(_) | E::TransfersRepository(_) => {
                ApiError::Internal(anyhow::anyhow!(err))
            }
        }
    }
}

impl From<FeedServiceError> for ApiError {
    fn from(err: FeedServiceError) -> Self {
        match err {
            FeedServiceError::Currencies(c) => ApiError::from(c),
            FeedServiceError::Repository(_) => ApiError::Internal(anyhow::anyhow!(err)),
        }
    }
}
