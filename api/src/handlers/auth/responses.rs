use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::models::{AuthenticatedUser, Session};

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    pub user_id: i64,
    pub username: String,
    pub session_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

impl From<(AuthenticatedUser, Session)> for LoginResponse {
    fn from((user, session): (AuthenticatedUser, Session)) -> Self {
        Self {
            user_id: user.user.id,
            username: user.user.username,
            session_id: session.id,
            expires_at: session.absolute_expires_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LogoutResponse {
    pub success: bool,
}
