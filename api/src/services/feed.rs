use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::broadcast::Receiver;

use crate::broadcaster::{FeedBroadcaster, FeedEvent};
use crate::models::{FeedEntry, Money};
use crate::repositories::transfers::{TransfersRepository, TransfersRepositoryError};
use crate::services::currencies::{CurrenciesService, CurrenciesServiceError};

#[derive(Debug, Error)]
pub enum FeedServiceError {
    #[error(transparent)]
    Repository(#[from] TransfersRepositoryError),

    #[error(transparent)]
    Currencies(#[from] CurrenciesServiceError),
}

#[async_trait]
pub trait FeedService: Send + Sync {
    async fn list_recent(&self, limit: i64) -> Result<Vec<FeedEntry>, FeedServiceError>;
    fn subscribe(&self) -> Receiver<FeedEvent>;
}

pub struct FeedServiceImpl {
    transfers: Arc<dyn TransfersRepository>,
    broadcaster: Arc<FeedBroadcaster>,
    currencies: Arc<dyn CurrenciesService>,
}

impl FeedServiceImpl {
    pub fn new(
        transfers: Arc<dyn TransfersRepository>,
        broadcaster: Arc<FeedBroadcaster>,
        currencies: Arc<dyn CurrenciesService>,
    ) -> Self {
        Self { transfers, broadcaster, currencies }
    }
}

#[async_trait]
impl FeedService for FeedServiceImpl {
    #[tracing::instrument(skip_all, fields(limit = limit))]
    async fn list_recent(&self, limit: i64) -> Result<Vec<FeedEntry>, FeedServiceError> {
        let entities = self.transfers.list_recent_feed(limit).await?;
        let mut out = Vec::with_capacity(entities.len());
        for e in entities {
            let currency = self.currencies.require(&e.currency_code).await?;
            out.push(FeedEntry {
                action_id: e.action_id,
                operation_id: e.operation_id,
                sender_username: e.sender_username,
                recipient_username: e.recipient_username,
                amount: Money::new(e.amount_minor_units, currency),
                created_at: e.created_at,
            });
        }
        Ok(out)
    }

    fn subscribe(&self) -> Receiver<FeedEvent> {
        self.broadcaster.subscribe()
    }
}
