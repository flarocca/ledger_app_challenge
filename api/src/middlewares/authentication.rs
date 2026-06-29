use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use crate::error::ApiError;
use crate::models::{AuthenticatedUser, Session};
use crate::state::AppState;

#[derive(Clone)]
pub struct AuthContext {
    pub user: AuthenticatedUser,
    pub session: Session,
}

pub async fn authentication_middleware(
    State(state): State<AppState>,
    jar: CookieJar,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let cookie = jar
        .get(&state.config.session.cookie_name)
        .ok_or(ApiError::Unauthorized)?;

    let session_id = Uuid::parse_str(cookie.value()).map_err(|_| ApiError::Unauthorized)?;

    let (user, session) = state.auth.validate_session(session_id).await?;

    req.extensions_mut().insert(AuthContext { user, session });

    Ok(next.run(req).await)
}
