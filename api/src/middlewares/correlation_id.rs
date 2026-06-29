use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::Instrument;
use uuid::Uuid;

use crate::error::{ApiError, ErrorDetail};
use crate::response::ApiResponse;

pub static REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Clone, Copy)]
pub struct RequestId(pub Uuid);

pub async fn correlation_id_middleware(mut req: Request, next: Next) -> Response {
    let header_value = req
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok());

    let is_mutation = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );

    let request_id = match header_value {
        Some(id) => id,
        None if is_mutation => {
            let err = ApiError::BadRequest("X-Request-Id header is required for mutations".into());
            let envelope = ApiResponse::<()>::error(
                ErrorDetail { code: err.code().to_string(), message: err.to_string() },
                None,
                None,
            );
            return (StatusCode::BAD_REQUEST, axum::Json(envelope)).into_response();
        }
        None => Uuid::new_v4(),
    };

    req.extensions_mut().insert(RequestId(request_id));

    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let span = tracing::info_span!(
        "request",
        request_id = %request_id,
        method = %method,
        path = %path,
    );

    let mut res = async move {
        tracing::info!(target: "ledger_api::request", "{method} {path}");
        next.run(req).await
    }
    .instrument(span)
    .await;

    if let Ok(hv) = HeaderValue::from_str(&request_id.to_string()) {
        res.headers_mut().insert(REQUEST_ID_HEADER.clone(), hv);
    }
    res
}
