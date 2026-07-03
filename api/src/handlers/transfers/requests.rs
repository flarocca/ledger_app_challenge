use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, Validate, ToSchema)]
pub struct TransferRecipientRequest {
    #[validate(length(min = 1, max = 64))]
    pub recipient_username: String,
    #[validate(length(min = 1, max = 32))]
    pub amount: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateTransferRequest {
    #[validate(length(min = 1, max = 32))]
    #[validate(nested)]
    pub recipients: Vec<TransferRecipientRequest>,
    #[validate(length(equal = 3))]
    pub currency: String,
}
