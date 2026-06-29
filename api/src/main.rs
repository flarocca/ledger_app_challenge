use std::sync::Arc;
use std::time::Duration;

use ledger_api::broadcaster::FeedBroadcaster;
use ledger_api::clock::{Clock, SystemClock};
use ledger_api::config::AppConfig;
use ledger_api::repositories::currencies::{CachedCurrenciesRepository, CurrenciesRepository, PgCurrenciesRepository};
use ledger_api::repositories::idempotency::{IdempotencyRepository, InMemoryIdempotencyRepository};
use ledger_api::repositories::sessions::{CachedSessionsRepository, PgSessionsRepository, SessionsRepository};
use ledger_api::repositories::transfers::{PgTransfersRepository, TransfersRepository};
use ledger_api::repositories::users::{CachedUsersRepository, PgUsersRepository, UsersRepository};
use ledger_api::router::build_router;
use ledger_api::services::auth::{AuthService, AuthServiceImpl};
use ledger_api::services::currencies::{CurrenciesService, CurrenciesServiceImpl};
use ledger_api::services::feed::{FeedService, FeedServiceImpl};
use ledger_api::services::idempotency::{IdempotencyService, IdempotencyServiceImpl};
use ledger_api::services::transfers::{TransfersService, TransfersServiceImpl};
use ledger_api::services::users::{UsersService, UsersServiceImpl};
use ledger_api::state::AppState;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    ledger_api::logging::init()?;
    let config = AppConfig::load_from_env();
    tracing::info!("starting ledger-api");

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.server.database_url)
        .await?;

    sqlx::migrate!("../migrations").run(&pool).await?;

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let broadcaster = Arc::new(FeedBroadcaster::new(config.feed.broadcast_capacity));

    let pg_users: Arc<dyn UsersRepository> = Arc::new(PgUsersRepository::new(pool.clone()));
    let users_repo: Arc<dyn UsersRepository> =
        Arc::new(CachedUsersRepository::new(pg_users, Duration::from_secs(60)));

    let pg_sessions: Arc<dyn SessionsRepository> = Arc::new(PgSessionsRepository::new(pool.clone()));
    let sessions_repo: Arc<dyn SessionsRepository> =
        Arc::new(CachedSessionsRepository::new(pg_sessions, Duration::from_secs(30)));

    let idempotency_repo: Arc<dyn IdempotencyRepository> = Arc::new(InMemoryIdempotencyRepository::new());

    let transfers_repo: Arc<dyn TransfersRepository> = Arc::new(PgTransfersRepository::new(pool.clone()));

    let pg_currencies: Arc<dyn CurrenciesRepository> = Arc::new(PgCurrenciesRepository::new(pool.clone()));
    let currencies_repo: Arc<dyn CurrenciesRepository> =
        Arc::new(CachedCurrenciesRepository::new(pg_currencies, Duration::from_secs(300)));
    let currencies: Arc<dyn CurrenciesService> = Arc::new(CurrenciesServiceImpl::new(currencies_repo));

    let auth: Arc<dyn AuthService> = Arc::new(AuthServiceImpl::new(
        users_repo.clone(),
        sessions_repo.clone(),
        clock.clone(),
        config.session.rolling_window_secs,
        config.session.absolute_secs,
    ));

    let users: Arc<dyn UsersService> = Arc::new(UsersServiceImpl::new(users_repo.clone()));
    let transfers: Arc<dyn TransfersService> = Arc::new(TransfersServiceImpl::new(
        transfers_repo.clone(),
        users_repo.clone(),
        broadcaster.clone(),
    ));
    let feed: Arc<dyn FeedService> = Arc::new(FeedServiceImpl::new(
        transfers_repo.clone(),
        broadcaster.clone(),
        currencies.clone(),
    ));
    let idempotency: Arc<dyn IdempotencyService> = Arc::new(IdempotencyServiceImpl::new(
        idempotency_repo,
        clock.clone(),
        config.session.idempotency_ttl_secs,
    ));

    let bind_address = config.server.bind_address();
    let state = AppState {
        config: Arc::new(config),
        clock,
        broadcaster,
        auth,
        users,
        transfers,
        feed,
        idempotency,
        currencies,
    };

    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    tracing::info!("listening on {bind_address}");
    axum::serve(listener, app).await?;
    Ok(())
}
