use axum::Extension;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, Response, StatusCode, header};
use axum::middleware::Next;

use crate::error::ApiError;
use crate::middlewares::authentication::AuthContext;
use crate::middlewares::correlation_id::RequestId;
use crate::models::CachedResponse;
use crate::state::AppState;

const MAX_CACHED_BODY_BYTES: usize = 1024 * 1024;

pub async fn idempotency_middleware(
    State(state): State<AppState>,
    Extension(RequestId(request_id)): Extension<RequestId>,
    Extension(auth): Extension<AuthContext>,
    req: Request,
    next: Next,
) -> Result<Response<Body>, ApiError> {
    if !is_mutation(req.method()) {
        return Ok(next.run(req).await);
    }

    let session_id = auth.session.id;

    if let Some(cached) = state.idempotency.get(request_id, session_id).await {
        return Ok(replay(cached));
    }

    let response = next.run(req).await;
    let (mut parts, body) = response.into_parts();

    if !parts.status.is_success() {
        return Ok(Response::from_parts(parts, body));
    }

    let bytes = to_bytes(body, MAX_CACHED_BODY_BYTES).await.map_err(|e| {
        ApiError::Internal(anyhow::anyhow!("response body too large to cache: {e}"))
    })?;

    parts
        .headers
        .insert(header::CONTENT_LENGTH, HeaderValue::from(bytes.len()));

    state
        .idempotency
        .put(
            request_id,
            session_id,
            CachedResponse {
                status_code: parts.status.as_u16(),
                headers: parts.headers.clone(),
                body: bytes.to_vec(),
            },
        )
        .await;

    Ok(Response::from_parts(parts, Body::from(bytes)))
}

fn is_mutation(method: &Method) -> bool {
    matches!(*method, Method::POST | Method::PUT | Method::PATCH | Method::DELETE)
}

fn replay(cached: CachedResponse) -> Response<Body> {
    let status = StatusCode::from_u16(cached.status_code).unwrap_or(StatusCode::OK);
    let mut resp = Response::builder()
        .status(status)
        .body(Body::from(cached.body))
        .expect("building response from cached idempotent entry");
    *resp.headers_mut() = cached.headers;
    resp
}
