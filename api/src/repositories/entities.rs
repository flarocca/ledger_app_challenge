use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct UserAccountEntity {
    pub user_id: i64,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub is_system: bool,
    pub account_id: i64,
    pub currency: String,
}

#[derive(Clone, Debug)]
pub struct BalanceEntity {
    pub account_id: i64,
    pub minor_units: i64,
}

#[derive(Clone, Debug)]
pub struct SessionEntity {
    pub id: Uuid,
    pub user_id: i64,
    pub created_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub rolling_expires_at: DateTime<Utc>,
    pub absolute_expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct CurrencyEntity {
    pub code: String,
    pub exponent: u8,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct TransferLegOutcomeEntity {
    pub recipient_account_id: i64,
    pub action_id: i64,
}

#[derive(Clone, Debug)]
pub struct TransferOutcomeEntity {
    pub operation_id: Uuid,
    pub sender_balance_minor_units: i64,
    pub created_at: DateTime<Utc>,
    pub legs: Vec<TransferLegOutcomeEntity>,
}

#[derive(Clone, Debug)]
pub struct FeedActionEntity {
    pub action_id: i64,
    pub operation_id: Uuid,
    pub sender_username: String,
    pub recipient_username: String,
    pub amount_minor_units: i64,
    pub currency_code: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct RecipientAccountEntity {
    pub user_id: i64,
    pub account_id: i64,
    pub currency_code: String,
}

#[derive(Clone, Debug)]
pub struct IdempotencyEntryEntity {
    pub status_code: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub expires_at: DateTime<Utc>,
}
