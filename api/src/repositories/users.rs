use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dashmap::DashMap;
use sqlx::PgPool;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::repositories::entities::UserAccountEntity;

#[derive(Debug, Error)]
pub enum UsersRepositoryError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[async_trait]
pub trait UsersRepository: Send + Sync {
    async fn find_by_username(&self, username: &str) -> Result<Option<UserAccountEntity>, UsersRepositoryError>;
    async fn find_by_id(&self, id: i64) -> Result<Option<UserAccountEntity>, UsersRepositoryError>;
    async fn get_balance(&self, account_id: i64) -> Result<Option<i64>, UsersRepositoryError>;
}

pub struct PgUsersRepository {
    pool: PgPool,
}

impl PgUsersRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UsersRepository for PgUsersRepository {
    #[tracing::instrument(skip_all, fields(username = %username))]
    async fn find_by_username(&self, username: &str) -> Result<Option<UserAccountEntity>, UsersRepositoryError> {
        let row = sqlx::query!(
            r#"
            SELECT u.id AS user_id, u.username, u.email, u.password_hash, u.is_system,
                   a.id AS account_id, a.currency::TEXT AS "currency!"
            FROM users u
            JOIN accounts a ON a.user_id = u.id
            WHERE u.username = $1 AND u.is_system = FALSE
            "#,
            username
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| UserAccountEntity {
            user_id: r.user_id,
            username: r.username,
            email: r.email,
            password_hash: r.password_hash,
            is_system: r.is_system,
            account_id: r.account_id,
            currency: r.currency,
        }))
    }

    #[tracing::instrument(skip_all, fields(user_id = id))]
    async fn find_by_id(&self, id: i64) -> Result<Option<UserAccountEntity>, UsersRepositoryError> {
        let row = sqlx::query!(
            r#"
            SELECT u.id AS user_id, u.username, u.email, u.password_hash, u.is_system,
                   a.id AS account_id, a.currency::TEXT AS "currency!"
            FROM users u
            JOIN accounts a ON a.user_id = u.id
            WHERE u.id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| UserAccountEntity {
            user_id: r.user_id,
            username: r.username,
            email: r.email,
            password_hash: r.password_hash,
            is_system: r.is_system,
            account_id: r.account_id,
            currency: r.currency,
        }))
    }

    #[tracing::instrument(skip_all, fields(account_id = account_id))]
    async fn get_balance(&self, account_id: i64) -> Result<Option<i64>, UsersRepositoryError> {
        Ok(sqlx::query_scalar!(
            r#"SELECT balance FROM account_balances WHERE account_id = $1"#,
            account_id
        )
        .fetch_optional(&self.pool)
        .await?)
    }
}

struct CacheEntry {
    value: UserAccountEntity,
    inserted_at: std::time::Instant,
}

pub struct CachedUsersRepository {
    inner: Arc<dyn UsersRepository>,
    by_username: DashMap<String, CacheEntry>,
    by_id: DashMap<i64, CacheEntry>,
    ttl: Duration,
    sweep: Mutex<std::time::Instant>,
}

impl CachedUsersRepository {
    pub fn new(inner: Arc<dyn UsersRepository>, ttl: Duration) -> Self {
        Self {
            inner,
            by_username: DashMap::new(),
            by_id: DashMap::new(),
            ttl,
            sweep: Mutex::new(std::time::Instant::now()),
        }
    }

    fn fresh(&self, e: &CacheEntry) -> bool {
        e.inserted_at.elapsed() < self.ttl
    }

    async fn maybe_sweep(&self) {
        let mut last = self.sweep.lock().await;
        if last.elapsed() < self.ttl {
            return;
        }
        self.by_username.retain(|_, e| self.fresh(e));
        self.by_id.retain(|_, e| self.fresh(e));
        *last = std::time::Instant::now();
    }
}

#[async_trait]
impl UsersRepository for CachedUsersRepository {
    async fn find_by_username(&self, username: &str) -> Result<Option<UserAccountEntity>, UsersRepositoryError> {
        self.maybe_sweep().await;
        if let Some(e) = self.by_username.get(username) {
            if self.fresh(&e) {
                return Ok(Some(e.value.clone()));
            }
        }
        let fetched = self.inner.find_by_username(username).await?;
        if let Some(u) = &fetched {
            let now = std::time::Instant::now();
            self.by_username.insert(username.to_string(), CacheEntry { value: u.clone(), inserted_at: now });
            self.by_id.insert(u.user_id, CacheEntry { value: u.clone(), inserted_at: now });
        }
        Ok(fetched)
    }

    async fn find_by_id(&self, id: i64) -> Result<Option<UserAccountEntity>, UsersRepositoryError> {
        self.maybe_sweep().await;
        if let Some(e) = self.by_id.get(&id) {
            if self.fresh(&e) {
                return Ok(Some(e.value.clone()));
            }
        }
        let fetched = self.inner.find_by_id(id).await?;
        if let Some(u) = &fetched {
            let now = std::time::Instant::now();
            self.by_id.insert(id, CacheEntry { value: u.clone(), inserted_at: now });
            self.by_username.insert(u.username.clone(), CacheEntry { value: u.clone(), inserted_at: now });
        }
        Ok(fetched)
    }

    async fn get_balance(&self, account_id: i64) -> Result<Option<i64>, UsersRepositoryError> {
        self.inner.get_balance(account_id).await
    }
}
