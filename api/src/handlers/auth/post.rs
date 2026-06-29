use axum::Extension;
use axum::extract::State;
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use validator::Validate;

use crate::error::ApiError;
use crate::handlers::auth::requests::LoginRequest;
use crate::handlers::auth::responses::{LoginResponse, LogoutResponse};
use crate::middlewares::authentication::AuthContext;
use crate::middlewares::correlation_id::RequestId;
use crate::response::ApiResponse;
use crate::state::AppState;

#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Logged in", body = LoginResponse),
        (status = 401, description = "Invalid credentials"),
    )
)]
#[tracing::instrument(skip_all, fields(username = %req.username))]
pub async fn login(
    State(state): State<AppState>,
    Extension(RequestId(request_id)): Extension<RequestId>,
    jar: CookieJar,
    axum::Json(req): axum::Json<LoginRequest>,
) -> Result<Response, ApiError> {
    req.validate()?;

    let (user, session) = state.auth.login(req.into()).await?;

    let mut cookie = Cookie::new(state.config.session.cookie_name.clone(), session.id.to_string());
    cookie.set_http_only(true);
    cookie.set_secure(state.config.session.cookie_secure);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path("/");
    cookie.set_max_age(time::Duration::seconds(state.config.session.absolute_secs));

    let body = LoginResponse::from((user, session));
    let envelope = ApiResponse::ok(body, Some(request_id));
    let mut response = axum::Json(envelope).into_response();
    response
        .headers_mut()
        .append(axum::http::header::SET_COOKIE, HeaderValue::from_str(&cookie.to_string()).unwrap());
    let _ = jar;
    Ok(response)
}

#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    responses((status = 200, description = "Logged out", body = LogoutResponse))
)]
#[tracing::instrument(skip_all, fields(user_id = auth.user.user.id))]
pub async fn logout(
    State(state): State<AppState>,
    Extension(RequestId(request_id)): Extension<RequestId>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Response, ApiError> {
    state.auth.logout(auth.session.id).await?;

    let mut cookie = Cookie::new(state.config.session.cookie_name.clone(), "");
    cookie.set_path("/");
    cookie.set_http_only(true);
    cookie.set_max_age(time::Duration::seconds(0));

    let envelope = ApiResponse::ok(LogoutResponse { success: true }, Some(request_id));
    let mut response = axum::Json(envelope).into_response();
    response
        .headers_mut()
        .append(axum::http::header::SET_COOKIE, HeaderValue::from_str(&cookie.to_string()).unwrap());
    Ok(response)
}
