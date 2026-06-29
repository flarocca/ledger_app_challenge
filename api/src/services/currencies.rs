use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

use crate::models::Currency;
use crate::repositories::currencies::{CurrenciesRepository, CurrenciesRepositoryError};

#[derive(Debug, Error)]
pub enum CurrenciesServiceError {
    #[error("unsupported currency '{0}'")]
    UnsupportedCurrency(String),

    #[error(transparent)]
    Repository(#[from] CurrenciesRepositoryError),
}

#[async_trait]
pub trait CurrenciesService: Send + Sync {
    async fn require(&self, code: &str) -> Result<Currency, CurrenciesServiceError>;
}

pub struct CurrenciesServiceImpl {
    repo: Arc<dyn CurrenciesRepository>,
}

impl CurrenciesServiceImpl {
    pub fn new(repo: Arc<dyn CurrenciesRepository>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl CurrenciesService for CurrenciesServiceImpl {
    #[tracing::instrument(skip_all, fields(code = %code))]
    async fn require(&self, code: &str) -> Result<Currency, CurrenciesServiceError> {
        self.repo
            .list_all()
            .await?
            .into_iter()
            .find(|c| c.code == code)
            .map(Currency::from)
            .ok_or_else(|| CurrenciesServiceError::UnsupportedCurrency(code.to_string()))
    }
}
