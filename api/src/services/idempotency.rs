use std::sync::Arc;

use async_trait::async_trait;
use chrono::Duration;
use uuid::Uuid;

use crate::clock::Clock;
use crate::repositories::entities::IdempotencyEntryEntity;
use crate::models::CachedResponse;
use crate::repositories::idempotency::IdempotencyRepository;

#[async_trait]
pub trait IdempotencyService: Send + Sync {
    async fn get(&self, request_id: Uuid, session_id: Uuid) -> Option<CachedResponse>;
    async fn put(&self, request_id: Uuid, session_id: Uuid, response: CachedResponse);
}

pub struct IdempotencyServiceImpl {
    repo: Arc<dyn IdempotencyRepository>,
    clock: Arc<dyn Clock>,
    ttl: Duration,
}

impl IdempotencyServiceImpl {
    pub fn new(repo: Arc<dyn IdempotencyRepository>, clock: Arc<dyn Clock>, ttl_secs: i64) -> Self {
        Self { repo, clock, ttl: Duration::seconds(ttl_secs) }
    }
}

#[async_trait]
impl IdempotencyService for IdempotencyServiceImpl {
    #[tracing::instrument(skip_all, fields(session_id = %session_id))]
    async fn get(&self, request_id: Uuid, session_id: Uuid) -> Option<CachedResponse> {
        self.repo
            .get(request_id, session_id, self.clock.now())
            .await
            .map(|entity| CachedResponse {
                status_code: entity.status_code,
                headers: entity.headers,
                body: entity.body,
            })
    }

    #[tracing::instrument(skip_all, fields(session_id = %session_id, status = response.status_code))]
    async fn put(&self, request_id: Uuid, session_id: Uuid, response: CachedResponse) {
        let entity = IdempotencyEntryEntity {
            status_code: response.status_code,
            headers: response.headers,
            body: response.body,
            expires_at: self.clock.now() + self.ttl,
        };
        self.repo.put(request_id, session_id, entity).await;
    }
}
