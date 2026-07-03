use crate::error::ApiError;
use crate::handlers::feed::responses::{FeedItem, FeedListResponse};
use crate::middlewares::authentication::AuthContext;
use crate::middlewares::correlation_id::RequestId;
use crate::response::ApiResponse;
use crate::state::AppState;
use axum::Extension;
use axum::extract::State;
use axum::response::IntoResponse;

#[utoipa::path(
    get,
    path = "/feed",
    tag = "feed",
    responses((status = 200, description = "Recent transfers", body = FeedListResponse))
)]
#[tracing::instrument(skip_all)]
pub async fn list(
    State(state): State<AppState>,
    Extension(RequestId(request_id)): Extension<RequestId>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<impl IntoResponse, ApiError> {
    let limit = state.config.feed.backfill_size as i64;
    let entries = state.feed.list_recent(limit).await?;
    let items = entries.into_iter().map(FeedItem::from).collect();
    Ok(axum::Json(ApiResponse::ok(
        FeedListResponse { items },
        Some(request_id),
    )))
}
