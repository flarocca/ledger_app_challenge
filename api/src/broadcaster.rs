use serde::Serialize;
use tokio::sync::broadcast;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::models::TransferResult;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct FeedEvent {
    pub operation_id: Uuid,
    pub sender_username: String,
    pub recipient_username: String,
    pub amount: String,
    pub currency: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<&TransferResult> for FeedEvent {
    fn from(r: &TransferResult) -> Self {
        Self {
            operation_id: r.operation_id,
            sender_username: r.sender_username.clone(),
            recipient_username: r.recipient_username.clone(),
            amount: r.amount.to_decimal_string(),
            currency: r.amount.currency.code.clone(),
            created_at: r.created_at,
        }
    }
}

pub struct FeedBroadcaster {
    tx: broadcast::Sender<FeedEvent>,
}

impl FeedBroadcaster {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, event: FeedEvent) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<FeedEvent> {
        self.tx.subscribe()
    }
}
