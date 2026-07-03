use axum::Router;
use axum::http::{HeaderName, HeaderValue, Method};
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use tower_http::cors::{AllowOrigin, CorsLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::handlers::auth::post as auth_post;
use crate::handlers::feed::{get as feed_get, stream as feed_stream};
use crate::handlers::transfers::post as transfers_post;
use crate::handlers::users::get as users_get;
use crate::middlewares::authentication::authentication_middleware;
use crate::middlewares::correlation_id::{REQUEST_ID_HEADER, correlation_id_middleware};
use crate::middlewares::idempotency::idempotency_middleware;
use crate::openapi::ApiDoc;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    let cors_origin = state.config.server.cors_allow_origin.clone();

    let public = Router::new().route("/auth/login", post(auth_post::login));

    let private = Router::new()
        .route("/auth/logout", post(auth_post::logout))
        .route("/users/me", get(users_get::me))
        .route("/transfers", post(transfers_post::create))
        .route("/feed", get(feed_get::list))
        .route("/feed/stream", get(feed_stream::stream))
        .layer(from_fn_with_state(state.clone(), idempotency_middleware))
        .layer(from_fn_with_state(state.clone(), authentication_middleware));

    let allowed_origin: HeaderValue = cors_origin
        .parse()
        .unwrap_or_else(|_| HeaderValue::from_static("http://localhost:3000"));

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(allowed_origin))
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            HeaderName::from_static("content-type"),
            REQUEST_ID_HEADER.clone(),
        ])
        .expose_headers([REQUEST_ID_HEADER.clone()]);

    Router::new()
        .merge(public)
        .merge(private)
        .route("/health", get(|| async { "ok" }))
        .merge(SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi()))
        .layer(axum::middleware::from_fn(correlation_id_middleware))
        .layer(cors)
        .with_state(state)
}
