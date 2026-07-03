use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

use crate::repositories::users::{UsersRepository, UsersRepositoryError};

#[derive(Debug, Error)]
pub enum UsersServiceError {
    #[error("account {0} not found")]
    AccountNotFound(i64),

    #[error(transparent)]
    Repository(#[from] UsersRepositoryError),
}

#[async_trait]
pub trait UsersService: Send + Sync {
    async fn get_balance(&self, account_id: i64) -> Result<i64, UsersServiceError>;
}

pub struct UsersServiceImpl {
    users: Arc<dyn UsersRepository>,
}

impl UsersServiceImpl {
    pub fn new(users: Arc<dyn UsersRepository>) -> Self {
        Self { users }
    }
}

#[async_trait]
impl UsersService for UsersServiceImpl {
    #[tracing::instrument(skip_all, fields(account_id = account_id))]
    async fn get_balance(&self, account_id: i64) -> Result<i64, UsersServiceError> {
        self.users
            .get_balance(account_id)
            .await?
            .ok_or(UsersServiceError::AccountNotFound(account_id))
    }
}
