use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

use crate::repositories::entities::SessionEntity;

#[derive(Debug, Error)]
pub enum SessionsRepositoryError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[async_trait]
pub trait SessionsRepository: Send + Sync {
    async fn create(
        &self,
        user_id: i64,
        created_at: DateTime<Utc>,
        rolling_expires_at: DateTime<Utc>,
        absolute_expires_at: DateTime<Utc>,
    ) -> Result<SessionEntity, SessionsRepositoryError>;

    async fn find_by_id(&self, id: Uuid) -> Result<Option<SessionEntity>, SessionsRepositoryError>;

    async fn touch(
        &self,
        id: Uuid,
        last_activity_at: DateTime<Utc>,
        rolling_expires_at: DateTime<Utc>,
    ) -> Result<(), SessionsRepositoryError>;

    async fn revoke(&self, id: Uuid, when: DateTime<Utc>) -> Result<(), SessionsRepositoryError>;
}

pub struct PgSessionsRepository {
    pool: PgPool,
}

impl PgSessionsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SessionsRepository for PgSessionsRepository {
    #[tracing::instrument(skip_all, fields(user_id = user_id))]
    async fn create(
        &self,
        user_id: i64,
        created_at: DateTime<Utc>,
        rolling_expires_at: DateTime<Utc>,
        absolute_expires_at: DateTime<Utc>,
    ) -> Result<SessionEntity, SessionsRepositoryError> {
        let id = Uuid::new_v4();
        sqlx::query!(
            r#"
            INSERT INTO sessions (id, user_id, created_at, last_activity_at, rolling_expires_at, absolute_expires_at)
            VALUES ($1, $2, $3, $3, $4, $5)
            "#,
            id, user_id, created_at, rolling_expires_at, absolute_expires_at
        )
        .execute(&self.pool)
        .await?;

        Ok(SessionEntity {
            id,
            user_id,
            created_at,
            last_activity_at: created_at,
            rolling_expires_at,
            absolute_expires_at,
            revoked_at: None,
        })
    }

    #[tracing::instrument(skip_all, fields(session_id = %id))]
    async fn find_by_id(&self, id: Uuid) -> Result<Option<SessionEntity>, SessionsRepositoryError> {
        let row = sqlx::query!(
            r#"
            SELECT id, user_id, created_at, last_activity_at, rolling_expires_at, absolute_expires_at, revoked_at
            FROM sessions WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| SessionEntity {
            id: r.id,
            user_id: r.user_id,
            created_at: r.created_at,
            last_activity_at: r.last_activity_at,
            rolling_expires_at: r.rolling_expires_at,
            absolute_expires_at: r.absolute_expires_at,
            revoked_at: r.revoked_at,
        }))
    }

    #[tracing::instrument(skip_all, fields(session_id = %id))]
    async fn touch(
        &self,
        id: Uuid,
        last_activity_at: DateTime<Utc>,
        rolling_expires_at: DateTime<Utc>,
    ) -> Result<(), SessionsRepositoryError> {
        sqlx::query!(
            r#"UPDATE sessions SET last_activity_at = $2, rolling_expires_at = $3 WHERE id = $1"#,
            id, last_activity_at, rolling_expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[tracing::instrument(skip_all, fields(session_id = %id))]
    async fn revoke(&self, id: Uuid, when: DateTime<Utc>) -> Result<(), SessionsRepositoryError> {
        sqlx::query!(
            r#"UPDATE sessions SET revoked_at = $2 WHERE id = $1 AND revoked_at IS NULL"#,
            id, when
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

struct CachedSession {
    entity: SessionEntity,
    inserted_at: std::time::Instant,
}

pub struct CachedSessionsRepository {
    inner: Arc<dyn SessionsRepository>,
    cache: DashMap<Uuid, CachedSession>,
    ttl: Duration,
}

impl CachedSessionsRepository {
    pub fn new(inner: Arc<dyn SessionsRepository>, ttl: Duration) -> Self {
        Self { inner, cache: DashMap::new(), ttl }
    }
}

#[async_trait]
impl SessionsRepository for CachedSessionsRepository {
    async fn create(
        &self,
        user_id: i64,
        created_at: DateTime<Utc>,
        rolling_expires_at: DateTime<Utc>,
        absolute_expires_at: DateTime<Utc>,
    ) -> Result<SessionEntity, SessionsRepositoryError> {
        let s = self.inner.create(user_id, created_at, rolling_expires_at, absolute_expires_at).await?;
        self.cache.insert(s.id, CachedSession { entity: s.clone(), inserted_at: std::time::Instant::now() });
        Ok(s)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<SessionEntity>, SessionsRepositoryError> {
        if let Some(e) = self.cache.get(&id) {
            if e.inserted_at.elapsed() < self.ttl {
                return Ok(Some(e.entity.clone()));
            }
        }
        let fetched = self.inner.find_by_id(id).await?;
        if let Some(s) = &fetched {
            self.cache.insert(s.id, CachedSession { entity: s.clone(), inserted_at: std::time::Instant::now() });
        }
        Ok(fetched)
    }

    async fn touch(
        &self,
        id: Uuid,
        last_activity_at: DateTime<Utc>,
        rolling_expires_at: DateTime<Utc>,
    ) -> Result<(), SessionsRepositoryError> {
        self.inner.touch(id, last_activity_at, rolling_expires_at).await?;
        if let Some(mut e) = self.cache.get_mut(&id) {
            e.entity.last_activity_at = last_activity_at;
            e.entity.rolling_expires_at = rolling_expires_at;
            e.inserted_at = std::time::Instant::now();
        }
        Ok(())
    }

    async fn revoke(&self, id: Uuid, when: DateTime<Utc>) -> Result<(), SessionsRepositoryError> {
        self.inner.revoke(id, when).await?;
        self.cache.remove(&id);
        Ok(())
    }
}
