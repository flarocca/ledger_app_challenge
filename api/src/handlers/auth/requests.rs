use serde::Deserialize;
use utoipa::ToSchema;
use validator::Validate;

use crate::models::Credentials;

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct LoginRequest {
    #[validate(length(min = 1, max = 64))]
    pub username: String,
    #[validate(length(min = 1, max = 256))]
    pub password: String,
}

impl From<LoginRequest> for Credentials {
    fn from(req: LoginRequest) -> Self {
        Credentials {
            username: req.username,
            password: req.password,
        }
    }
}
