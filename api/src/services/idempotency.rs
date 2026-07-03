use std::sync::Arc;

use async_trait::async_trait;
use chrono::Duration;
use uuid::Uuid;

use crate::clock::Clock;
use crate::models::CachedResponse;
use crate::repositories::entities::IdempotencyEntryEntity;
use crate::repositories::idempotency::{
    AcquireOutcome as RepoAcquireOutcome, IdempotencyRepository,
};

/// Guard handed out by `IdempotencyService::acquire` when the slot has been
/// reserved for this caller. The type system uses it to enforce the required
/// ordering: `put` is only reachable through a `Reservation` (so `acquire`
/// must have run first), and finalization goes through `release`, which
/// consumes the guard so it can't be reused.
///
/// If a `Reservation` is dropped without `release`, the slot leaks as
/// `Pending` until process restart — Drop logs a warning so the bug is
/// visible in traces.
pub struct Reservation {
    request_id: Uuid,
    session_id: Uuid,
    staged: Option<CachedResponse>,
    consumed: bool,
}

impl Reservation {
    pub fn put(&mut self, response: CachedResponse) {
        self.staged = Some(response);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.consumed {
            tracing::warn!(
                request_id = %self.request_id,
                session_id = %self.session_id,
                "idempotency reservation dropped without release; slot will stay pending",
            );
        }
    }
}

pub enum AcquireOutcome {
    Cached(CachedResponse),
    Reserved(Reservation),
    InProgress,
}

#[async_trait]
pub trait IdempotencyService: Send + Sync {
    async fn acquire(&self, request_id: Uuid, session_id: Uuid) -> AcquireOutcome;
    async fn release(&self, reservation: Reservation);
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
    async fn acquire(&self, request_id: Uuid, session_id: Uuid) -> AcquireOutcome {
        match self.repo.acquire(request_id, session_id, self.clock.now()).await {
            RepoAcquireOutcome::Cached(entity) => AcquireOutcome::Cached(CachedResponse {
                status_code: entity.status_code,
                headers: entity.headers,
                body: entity.body,
            }),
            RepoAcquireOutcome::Reserved => AcquireOutcome::Reserved(Reservation {
                request_id,
                session_id,
                staged: None,
                consumed: false,
            }),
            RepoAcquireOutcome::InProgress => AcquireOutcome::InProgress,
        }
    }

    #[tracing::instrument(
        skip_all,
        fields(
            session_id = %reservation.session_id,
            committed = reservation.staged.is_some(),
        ),
    )]
    async fn release(&self, mut reservation: Reservation) {
        reservation.consumed = true;
        let request_id = reservation.request_id;
        let session_id = reservation.session_id;
        match reservation.staged.take() {
            Some(response) => {
                let entity = IdempotencyEntryEntity {
                    status_code: response.status_code,
                    headers: response.headers,
                    body: response.body,
                    expires_at: self.clock.now() + self.ttl,
                };
                self.repo.put(request_id, session_id, entity).await;
            }
            None => {
                self.repo.release(request_id, session_id).await;
            }
        }
    }
}
