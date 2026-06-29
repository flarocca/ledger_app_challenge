use axum::Extension;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::error::ApiError;
use crate::handlers::users::responses::{MeResponse, UserResponse};
use crate::middlewares::authentication::AuthContext;
use crate::middlewares::correlation_id::RequestId;
use crate::models::Money;
use crate::response::ApiResponse;
use crate::state::AppState;

#[derive(Debug, Deserialize, IntoParams)]
pub struct SearchQuery {
    pub username: String,
}

#[utoipa::path(
    get,
    path = "/users/search",
    tag = "users",
    params(SearchQuery),
    responses(
        (status = 200, description = "User lookup result", body = UserResponse),
        (status = 404, description = "User not found"),
    )
)]
#[tracing::instrument(skip_all, fields(username = %q.username))]
pub async fn search(
    State(state): State<AppState>,
    Extension(RequestId(request_id)): Extension<RequestId>,
    Extension(_auth): Extension<AuthContext>,
    Query(q): Query<SearchQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let found = state.users.find_by_username(&q.username).await?;
    let body = UserResponse::from(found);
    Ok(axum::Json(ApiResponse::ok(body, Some(request_id))))
}

#[utoipa::path(
    get,
    path = "/users/me",
    tag = "users",
    responses((status = 200, description = "Current user profile and balance", body = MeResponse))
)]
#[tracing::instrument(skip_all, fields(user_id = auth.user.user.id))]
pub async fn me(
    State(state): State<AppState>,
    Extension(RequestId(request_id)): Extension<RequestId>,
    Extension(auth): Extension<AuthContext>,
) -> Result<impl IntoResponse, ApiError> {
    let minor_units = state.users.get_balance(auth.user.account.id).await?;
    let currency = state.currencies.require(&auth.user.account.currency).await?;
    let balance = Money::new(minor_units, currency);
    let body = MeResponse::from((auth.user, balance));
    Ok(axum::Json(ApiResponse::ok(body, Some(request_id))))
}
