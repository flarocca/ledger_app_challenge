use crate::models::FeedEntry;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct FeedItem {
    pub id: i64,
    pub operation_id: uuid::Uuid,
    pub sender_username: String,
    pub recipient_username: String,
    pub amount: String,
    pub currency: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<FeedEntry> for FeedItem {
    fn from(entry: FeedEntry) -> Self {
        let currency = entry.amount.currency.code.clone();
        Self {
            id: entry.action_id,
            operation_id: entry.operation_id,
            sender_username: entry.sender_username,
            recipient_username: entry.recipient_username,
            amount: entry.amount.to_decimal_string(),
            currency,
            created_at: entry.created_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FeedListResponse {
    pub items: Vec<FeedItem>,
}
