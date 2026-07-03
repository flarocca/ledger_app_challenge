use crate::models::TransferResult;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TransferLegResponse {
    pub action_id: i64,
    pub recipient_username: String,
    pub amount: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TransferResponse {
    pub operation_id: Uuid,
    pub sender_username: String,
    pub sender_balance_after: String,
    pub currency: String,
    pub created_at: DateTime<Utc>,
    pub transfers: Vec<TransferLegResponse>,
}

impl From<TransferResult> for TransferResponse {
    fn from(r: TransferResult) -> Self {
        let currency_code = r.currency.code.clone();
        Self {
            operation_id: r.operation_id,
            sender_username: r.sender_username,
            sender_balance_after: r.sender_balance.to_decimal_string(),
            currency: currency_code,
            created_at: r.created_at,
            transfers: r
                .legs
                .into_iter()
                .map(|leg| TransferLegResponse {
                    action_id: leg.action_id,
                    recipient_username: leg.recipient_username,
                    amount: leg.amount.to_decimal_string(),
                })
                .collect(),
        }
    }
}
