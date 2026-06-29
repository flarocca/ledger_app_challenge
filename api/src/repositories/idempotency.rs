use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use uuid::Uuid;

use crate::repositories::entities::IdempotencyEntryEntity;

#[async_trait]
pub trait IdempotencyRepository: Send + Sync {
    async fn get(
        &self,
        request_id: Uuid,
        session_id: Uuid,
        now: DateTime<Utc>,
    ) -> Option<IdempotencyEntryEntity>;

    async fn put(
        &self,
        request_id: Uuid,
        session_id: Uuid,
        entry: IdempotencyEntryEntity,
    );
}

pub struct InMemoryIdempotencyRepository {
    cache: DashMap<(Uuid, Uuid), IdempotencyEntryEntity>,
    sweep_interval: Duration,
    last_sweep: Mutex<Instant>,
}

impl InMemoryIdempotencyRepository {
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
            sweep_interval: Duration::from_secs(60),
            last_sweep: Mutex::new(Instant::now()),
        }
    }

    fn maybe_sweep(&self, now: DateTime<Utc>) {
        let mut last = match self.last_sweep.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if last.elapsed() < self.sweep_interval {
            return;
        }
        self.cache.retain(|_, entry| entry.expires_at > now);
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
    async fn get(
        &self,
        request_id: Uuid,
        session_id: Uuid,
        now: DateTime<Utc>,
    ) -> Option<IdempotencyEntryEntity> {
        let key = (request_id, session_id);
        let hit = self.cache.get(&key).and_then(|e| {
            (e.expires_at > now).then(|| e.clone())
        });
        self.maybe_sweep(now);
        hit
    }

    async fn put(
        &self,
        request_id: Uuid,
        session_id: Uuid,
        entry: IdempotencyEntryEntity,
    ) {
        self.cache.insert((request_id, session_id), entry);
    }
}
