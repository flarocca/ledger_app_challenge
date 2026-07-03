use serde::Serialize;
use tokio::sync::broadcast;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct FeedEvent {
    pub id: i64,
    pub operation_id: Uuid,
    pub sender_username: String,
    pub recipient_username: String,
    pub amount: String,
    pub currency: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
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
