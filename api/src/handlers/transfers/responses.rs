use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::models::TransferResult;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TransferResponse {
    pub operation_id: Uuid,
    pub sender_username: String,
    pub recipient_username: String,
    pub amount: String,
    pub currency: String,
    pub sender_balance_after: String,
    pub created_at: DateTime<Utc>,
}

impl From<TransferResult> for TransferResponse {
    fn from(r: TransferResult) -> Self {
        let currency_code = r.amount.currency.code.clone();
        Self {
            operation_id: r.operation_id,
            sender_username: r.sender_username,
            recipient_username: r.recipient_username,
            amount: r.amount.to_decimal_string(),
            currency: currency_code,
            sender_balance_after: r.sender_balance.to_decimal_string(),
            created_at: r.created_at,
        }
    }
}
