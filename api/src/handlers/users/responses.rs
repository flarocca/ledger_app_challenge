use crate::models::{AuthenticatedUser, Money};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    pub user_id: i64,
    pub username: String,
    pub email: String,
    pub balance: String,
    pub currency: String,
}

impl From<(AuthenticatedUser, Money)> for MeResponse {
    fn from((user, balance): (AuthenticatedUser, Money)) -> Self {
        Self {
            user_id: user.user.id,
            username: user.user.username,
            email: user.user.email,
            balance: balance.to_decimal_string(),
            currency: balance.currency.code,
        }
    }
}
