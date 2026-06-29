use serde::Serialize;
use utoipa::ToSchema;

use crate::models::FeedEntry;

#[derive(Debug, Serialize, ToSchema)]
pub struct FeedItem {
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
