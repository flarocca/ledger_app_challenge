use std::net::SocketAddr;
use std::sync::{Arc, Once};
use std::time::Duration;

static DOTENV: Once = Once::new();

fn ensure_dotenv() {
    DOTENV.call_once(|| {
        let _ = dotenvy::dotenv();
    });
}

use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
use argon2::Argon2;
use ledger_api::broadcaster::FeedBroadcaster;
use ledger_api::clock::{Clock, SystemClock};
use ledger_api::config::AppConfig;
use ledger_api::repositories::currencies::{CachedCurrenciesRepository, CurrenciesRepository, PgCurrenciesRepository};
use ledger_api::repositories::idempotency::{IdempotencyRepository, InMemoryIdempotencyRepository};
use ledger_api::repositories::sessions::{PgSessionsRepository, SessionsRepository};
use ledger_api::repositories::transfers::{PgTransfersRepository, TransfersRepository};
use ledger_api::repositories::users::{PgUsersRepository, UsersRepository};
use ledger_api::router::build_router;
use ledger_api::services::auth::{AuthService, AuthServiceImpl};
use ledger_api::services::currencies::{CurrenciesService, CurrenciesServiceImpl};
use ledger_api::services::feed::{FeedService, FeedServiceImpl};
use ledger_api::services::idempotency::{IdempotencyService, IdempotencyServiceImpl};
use ledger_api::services::transfers::{TransfersService, TransfersServiceImpl};
use ledger_api::services::users::{UsersService, UsersServiceImpl};
use ledger_api::state::AppState;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

pub const PASSWORD: &str = "password123";

pub fn database_url() -> String {
    ensure_dotenv();
    std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://ledger:ledger@localhost:5432/ledger".into())
}

pub async fn fresh_pool() -> PgPool {
    ensure_dotenv();
    let pool = PgPoolOptions::new().max_connections(8).connect(&database_url()).await.unwrap();
    sqlx::query("DROP SCHEMA public CASCADE").execute(&pool).await.ok();
    sqlx::query("CREATE SCHEMA public").execute(&pool).await.unwrap();
    sqlx::migrate!("../migrations").run(&pool).await.unwrap();
    seed_users(&pool).await;
    pool
}

pub async fn seed_users(pool: &PgPool) {
    let mut tx = pool.begin().await.unwrap();

    sqlx::query(
        r#"INSERT INTO currencies (code, exponent, name) VALUES ('USD', 2, 'United States Dollar') ON CONFLICT (code) DO NOTHING"#,
    )
    .execute(&mut *tx)
    .await
    .unwrap();

    let hash = |p: &str| {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default().hash_password(p.as_bytes(), &salt).unwrap().to_string()
    };

    let treasury_hash = hash("treasury-unused");
    let treasury_user_id: i64 = sqlx::query_scalar(
        r#"INSERT INTO users (username, email, password_hash, is_system) VALUES ('treasury', 'treasury@system.local', $1, TRUE) RETURNING id"#,
    )
    .bind(treasury_hash)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let treasury_account_id: i64 = sqlx::query_scalar(
        r#"INSERT INTO accounts (user_id, currency) VALUES ($1, 'USD') RETURNING id"#,
    )
    .bind(treasury_user_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO account_balances (account_id, currency, balance) VALUES ($1, 'USD', 0)"#,
    )
    .bind(treasury_account_id)
    .execute(&mut *tx)
    .await
    .unwrap();

    let users = ["alice", "bob", "carol", "dave"];
    let mut account_ids = Vec::new();
    for username in &users {
        let h = hash(PASSWORD);
        let user_id: i64 = sqlx::query_scalar(
            r#"INSERT INTO users (username, email, password_hash, is_system) VALUES ($1, $2, $3, FALSE) RETURNING id"#,
        )
        .bind(username)
        .bind(format!("{username}@example.com"))
        .bind(h)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        let account_id: i64 = sqlx::query_scalar(
            r#"INSERT INTO accounts (user_id, currency) VALUES ($1, 'USD') RETURNING id"#,
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            r#"INSERT INTO account_balances (account_id, currency, balance) VALUES ($1, 'USD', 0)"#,
        )
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .unwrap();
        account_ids.push(account_id);
    }

    sqlx::query("SELECT sp_genesis_issue($1, $2, $3, 'USD'::CHAR(3), $4)")
        .bind(treasury_account_id)
        .bind(&account_ids[..])
        .bind(100_000i64)
        .bind(Uuid::new_v4())
        .execute(&mut *tx)
        .await
        .unwrap();

    tx.commit().await.unwrap();
}

pub struct TestApp {
    pub base_url: String,
    pub pool: PgPool,
    pub _handle: tokio::task::JoinHandle<()>,
}

pub async fn spawn_app() -> TestApp {
    let pool = fresh_pool().await;
    let config = AppConfig::load_from_env();

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let broadcaster = Arc::new(FeedBroadcaster::new(config.feed.broadcast_capacity));

    let users_repo: Arc<dyn UsersRepository> = Arc::new(PgUsersRepository::new(pool.clone()));
    let sessions_repo: Arc<dyn SessionsRepository> = Arc::new(PgSessionsRepository::new(pool.clone()));
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

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    TestApp { base_url: format!("http://{addr}"), pool, _handle: handle }
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .unwrap()
}

pub async fn login(client: &reqwest::Client, base_url: &str, username: &str) -> serde_json::Value {
    let resp = client
        .post(format!("{base_url}/auth/login"))
        .header("x-request-id", Uuid::new_v4().to_string())
        .json(&serde_json::json!({ "username": username, "password": PASSWORD }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "login failed for {username}: {}", resp.status());
    resp.json().await.unwrap()
}

pub async fn balance(pool: &PgPool, username: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT ab.balance
        FROM users u
        JOIN accounts a ON a.user_id = u.id
        JOIN account_balances ab ON ab.account_id = a.id
        WHERE u.username = $1
        "#,
    )
    .bind(username)
    .fetch_one(pool)
    .await
    .unwrap()
}

pub async fn sum_all_balances(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>(r#"SELECT COALESCE(SUM(balance), 0)::BIGINT FROM account_balances"#)
        .fetch_one(pool)
        .await
        .unwrap()
}
