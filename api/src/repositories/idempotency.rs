use crate::repositories::entities::IdempotencyEntryEntity;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub enum AcquireOutcome {
    Cached(IdempotencyEntryEntity),
    Reserved,
    InProgress,
}

#[async_trait]
pub trait IdempotencyRepository: Send + Sync {
    async fn acquire(
        &self,
        request_id: Uuid,
        session_id: Uuid,
        now: DateTime<Utc>,
    ) -> AcquireOutcome;

    async fn put(&self, request_id: Uuid, session_id: Uuid, entry: IdempotencyEntryEntity);

    async fn release(&self, request_id: Uuid, session_id: Uuid);
}

enum Slot {
    Pending,
    Complete(IdempotencyEntryEntity),
}

pub struct InMemoryIdempotencyRepository {
    cache: DashMap<(Uuid, Uuid), Slot>,
    eviction_interval: Duration,
    last_eviction: Mutex<Instant>,
}

impl InMemoryIdempotencyRepository {
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
            eviction_interval: Duration::from_secs(60),
            last_eviction: Mutex::new(Instant::now()),
        }
    }

    fn evict_expired(&self, now: DateTime<Utc>) {
        let mut last = match self.last_eviction.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if last.elapsed() < self.eviction_interval {
            return;
        }
        self.cache.retain(|_, slot| match slot {
            Slot::Complete(entry) => entry.expires_at > now,
            Slot::Pending => true,
        });
        *last = Instant::now();
    }
}

impl Default for InMemoryIdempotencyRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IdempotencyRepository for InMemoryIdempotencyRepository {
    async fn acquire(
        &self,
        request_id: Uuid,
        session_id: Uuid,
        now: DateTime<Utc>,
    ) -> AcquireOutcome {
        let key = (request_id, session_id);

        let outcome = match self.cache.entry(key) {
            Entry::Occupied(mut occ) => {
                let outcome = match occ.get() {
                    Slot::Complete(e) if e.expires_at > now => AcquireOutcome::Cached(e.clone()),
                    Slot::Complete(_) => AcquireOutcome::Reserved,
                    Slot::Pending => AcquireOutcome::InProgress,
                };
                if matches!(outcome, AcquireOutcome::Reserved) {
                    occ.insert(Slot::Pending);
                }
                outcome
            }
            Entry::Vacant(vac) => {
                vac.insert(Slot::Pending);
                AcquireOutcome::Reserved
            }
        };
        self.evict_expired(now);
        outcome
    }

    async fn put(&self, request_id: Uuid, session_id: Uuid, entry: IdempotencyEntryEntity) {
        if let Entry::Occupied(mut occ) = self.cache.entry((request_id, session_id))
            && matches!(occ.get(), Slot::Pending)
        {
            occ.insert(Slot::Complete(entry));
        }
    }

    async fn release(&self, request_id: Uuid, session_id: Uuid) {
        self.cache.remove_if(&(request_id, session_id), |_, slot| {
            matches!(slot, Slot::Pending)
        });
    }
}
