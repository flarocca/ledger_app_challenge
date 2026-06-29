use std::sync::Arc;

use argon2::password_hash::{PasswordHash, PasswordVerifier, SaltString, rand_core::OsRng};
use argon2::{Argon2, PasswordHasher};
use async_trait::async_trait;
use chrono::Duration;
use thiserror::Error;
use uuid::Uuid;

use crate::clock::Clock;
use crate::models::{AuthenticatedUser, Credentials, Session};
use crate::repositories::sessions::{SessionsRepository, SessionsRepositoryError};
use crate::repositories::users::{UsersRepository, UsersRepositoryError};

#[derive(Debug, Error)]
pub enum AuthServiceError {
    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("invalid or expired session")]
    InvalidSession,

    #[error("password hash error: {0}")]
    HashError(String),

    #[error(transparent)]
    UsersRepository(#[from] UsersRepositoryError),

    #[error(transparent)]
    SessionsRepository(#[from] SessionsRepositoryError),
}

#[async_trait]
pub trait AuthService: Send + Sync {
    async fn login(&self, creds: Credentials) -> Result<(AuthenticatedUser, Session), AuthServiceError>;
    async fn validate_session(&self, session_id: Uuid) -> Result<(AuthenticatedUser, Session), AuthServiceError>;
    async fn logout(&self, session_id: Uuid) -> Result<(), AuthServiceError>;
}

pub struct AuthServiceImpl {
    users: Arc<dyn UsersRepository>,
    sessions: Arc<dyn SessionsRepository>,
    clock: Arc<dyn Clock>,
    rolling_window: Duration,
    absolute_window: Duration,
}

impl AuthServiceImpl {
    pub fn new(
        users: Arc<dyn UsersRepository>,
        sessions: Arc<dyn SessionsRepository>,
        clock: Arc<dyn Clock>,
        rolling_window_secs: i64,
        absolute_window_secs: i64,
    ) -> Self {
        Self {
            users,
            sessions,
            clock,
            rolling_window: Duration::seconds(rolling_window_secs),
            absolute_window: Duration::seconds(absolute_window_secs),
        }
    }

    pub fn hash_password(password: &str) -> Result<String, AuthServiceError> {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| AuthServiceError::HashError(e.to_string()))
    }

    fn verify_password(&self, password: &str, hash: &str) -> bool {
        let parsed = match PasswordHash::new(hash) {
            Ok(p) => p,
            Err(_) => return false,
        };
        Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
    }
}

#[async_trait]
impl AuthService for AuthServiceImpl {
    #[tracing::instrument(skip_all, fields(username = %creds.username))]
    async fn login(&self, creds: Credentials) -> Result<(AuthenticatedUser, Session), AuthServiceError> {
        let user: AuthenticatedUser = self
            .users
            .find_by_username(&creds.username)
            .await?
            .ok_or(AuthServiceError::InvalidCredentials)?
            .into();
        if user.user.is_system {
            return Err(AuthServiceError::InvalidCredentials);
        }
        if !self.verify_password(&creds.password, &user.user.password_hash) {
            return Err(AuthServiceError::InvalidCredentials);
        }
        let now = self.clock.now();
        let session: Session = self
            .sessions
            .create(user.user.id, now, now + self.rolling_window, now + self.absolute_window)
            .await?
            .into();
        Ok((user, session))
    }

    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    async fn validate_session(&self, session_id: Uuid) -> Result<(AuthenticatedUser, Session), AuthServiceError> {
        let mut session: Session = self
            .sessions
            .find_by_id(session_id)
            .await?
            .ok_or(AuthServiceError::InvalidSession)?
            .into();
        let now = self.clock.now();
        if !session.is_active(now) {
            return Err(AuthServiceError::InvalidSession);
        }
        let user: AuthenticatedUser = self
            .users
            .find_by_id(session.user_id)
            .await?
            .ok_or(AuthServiceError::InvalidSession)?
            .into();

        let new_rolling = now + self.rolling_window;
        let capped_rolling = std::cmp::min(new_rolling, session.absolute_expires_at);
        self.sessions.touch(session.id, now, capped_rolling).await?;
        session.last_activity_at = now;
        session.rolling_expires_at = capped_rolling;

        Ok((user, session))
    }

    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    async fn logout(&self, session_id: Uuid) -> Result<(), AuthServiceError> {
        let now = self.clock.now();
        self.sessions.revoke(session_id, now).await?;
        Ok(())
    }
}
