mod common;

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use ledger_api::clock::{Clock, TestClock};
use ledger_api::models::Credentials;
use ledger_api::repositories::sessions::{PgSessionsRepository, SessionsRepository};
use ledger_api::repositories::users::{PgUsersRepository, UsersRepository};
use ledger_api::services::auth::{AuthService, AuthServiceImpl};
use serial_test::serial;

use common::{fresh_pool, PASSWORD};

fn creds(username: &str) -> Credentials {
    Credentials { username: username.into(), password: PASSWORD.into() }
}

const ROLLING_SECS: i64 = 24 * 60 * 60;
const ABSOLUTE_SECS: i64 = 30 * 24 * 60 * 60;

fn build_auth(pool: sqlx::PgPool, clock: Arc<dyn Clock>) -> Arc<dyn AuthService> {
    let users_repo: Arc<dyn UsersRepository> = Arc::new(PgUsersRepository::new(pool.clone()));
    let sessions_repo: Arc<dyn SessionsRepository> = Arc::new(PgSessionsRepository::new(pool));
    Arc::new(AuthServiceImpl::new(users_repo, sessions_repo, clock, ROLLING_SECS, ABSOLUTE_SECS))
}

#[tokio::test]
#[serial]
async fn rolling_window_extends_on_activity() {
    let pool = fresh_pool().await;
    let clock = TestClock::new(Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap());
    let auth = build_auth(pool, Arc::new(clock.clone()));

    let (_, session) = auth.login(creds("alice")).await.unwrap();
    let original_rolling = session.rolling_expires_at;

    clock.advance(Duration::hours(20));
    let (_, refreshed) = auth.validate_session(session.id).await.unwrap();

    assert!(
        refreshed.rolling_expires_at > original_rolling,
        "rolling expiration should extend after activity inside the window"
    );
    assert_eq!(refreshed.rolling_expires_at, clock.now() + Duration::seconds(ROLLING_SECS));
}

#[tokio::test]
#[serial]
async fn session_expires_past_rolling_window_without_activity() {
    let pool = fresh_pool().await;
    let clock = TestClock::new(Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap());
    let auth = build_auth(pool, Arc::new(clock.clone()));

    let (_, session) = auth.login(creds("alice")).await.unwrap();
    clock.advance(Duration::hours(25));

    let result = auth.validate_session(session.id).await;
    assert!(result.is_err(), "validating after the rolling window must fail");
}

#[tokio::test]
#[serial]
async fn session_expires_past_absolute_window_even_with_activity() {
    let pool = fresh_pool().await;
    let clock = TestClock::new(Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap());
    let auth = build_auth(pool, Arc::new(clock.clone()));

    let (_, session) = auth.login(creds("alice")).await.unwrap();

    for _ in 0..30 {
        clock.advance(Duration::hours(20));
        if let Err(_) = auth.validate_session(session.id).await {
            break;
        }
    }

    clock.advance(Duration::days(2));
    let result = auth.validate_session(session.id).await;
    assert!(
        result.is_err(),
        "validating past the 30d absolute window must fail even with continuous activity"
    );
}

#[tokio::test]
#[serial]
async fn logout_invalidates_session_immediately() {
    let pool = fresh_pool().await;
    let clock = TestClock::new(Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap());
    let auth = build_auth(pool, Arc::new(clock.clone()));

    let (_, session) = auth.login(creds("alice")).await.unwrap();
    auth.logout(session.id).await.unwrap();

    let result = auth.validate_session(session.id).await;
    assert!(result.is_err(), "session should be unusable after logout");
}
