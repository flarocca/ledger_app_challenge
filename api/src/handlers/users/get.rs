use crate::error::ApiError;
use crate::handlers::users::responses::MeResponse;
use crate::middlewares::authentication::AuthContext;
use crate::middlewares::correlation_id::RequestId;
use crate::models::Money;
use crate::response::ApiResponse;
use crate::state::AppState;
use axum::Extension;
use axum::extract::State;
use axum::response::IntoResponse;

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
    let currency = state
        .currencies
        .require(&auth.user.account.currency)
        .await?;
    let balance = Money::new(minor_units, currency);
    let body = MeResponse::from((auth.user, balance));
    Ok(axum::Json(ApiResponse::ok(body, Some(request_id))))
}
