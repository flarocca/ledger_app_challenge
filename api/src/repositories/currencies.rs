use crate::repositories::entities::CurrencyEntity;
use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CurrenciesRepositoryError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[async_trait]
pub trait CurrenciesRepository: Send + Sync {
    async fn list_all(&self) -> Result<Vec<CurrencyEntity>, CurrenciesRepositoryError>;
}

pub struct PgCurrenciesRepository {
    pool: PgPool,
}

impl PgCurrenciesRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CurrenciesRepository for PgCurrenciesRepository {
    #[tracing::instrument(skip_all)]
    async fn list_all(&self) -> Result<Vec<CurrencyEntity>, CurrenciesRepositoryError> {
        let rows = sqlx::query!(
            r#"SELECT code::TEXT AS "code!", exponent, name FROM currencies ORDER BY code"#
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CurrencyEntity {
                code: r.code,
                exponent: r.exponent as u8,
                name: r.name,
            })
            .collect())
    }
}

struct CacheSnapshot {
    list: Vec<CurrencyEntity>,
    inserted_at: Instant,
}

pub struct CachedCurrenciesRepository {
    inner: Arc<dyn CurrenciesRepository>,
    cache: Mutex<Option<CacheSnapshot>>,
    ttl: Duration,
}

impl CachedCurrenciesRepository {
    pub fn new(inner: Arc<dyn CurrenciesRepository>, ttl: Duration) -> Self {
        Self {
            inner,
            cache: Mutex::new(None),
            ttl,
        }
    }
}

#[async_trait]
impl CurrenciesRepository for CachedCurrenciesRepository {
    #[tracing::instrument(skip_all)]
    async fn list_all(&self) -> Result<Vec<CurrencyEntity>, CurrenciesRepositoryError> {
        {
            let guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(snapshot) = guard.as_ref()
                && snapshot.inserted_at.elapsed() < self.ttl
            {
                return Ok(snapshot.list.clone());
            }
        }

        let list = self.inner.list_all().await?;

        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(CacheSnapshot {
            list: list.clone(),
            inserted_at: Instant::now(),
        });
        Ok(list)
    }
}
