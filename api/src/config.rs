use config::{Config, Environment};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub database_url: String,
    pub port: u16,
    pub cors_allow_origin: String,
}

impl ServerConfig {
    const CONFIG_PREFIX: &'static str = "SERVER_CONFIG";
    const HOST: &'static str = "0.0.0.0";

    pub fn load_from_env() -> Self {
        Config::builder()
            .set_default("database_url", "postgres://ledger:ledger@localhost:5432/ledger")
            .unwrap()
            .set_default("port", 4000)
            .unwrap()
            .set_default("cors_allow_origin", "http://localhost:3000")
            .unwrap()
            .add_source(Environment::with_prefix(Self::CONFIG_PREFIX).separator("__"))
            .build()
            .unwrap()
            .try_deserialize::<ServerConfig>()
            .expect("Failed to deserialize Server config")
    }

    pub fn bind_address(&self) -> String {
        format!("{}:{}", Self::HOST, self.port)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub cookie_name: String,
    pub cookie_secure: bool,
    pub rolling_window_secs: i64,
    pub absolute_secs: i64,
    pub idempotency_ttl_secs: i64,
}

impl SessionConfig {
    const CONFIG_PREFIX: &'static str = "SESSION_CONFIG";

    pub fn load_from_env() -> Self {
        Config::builder()
            .set_default("cookie_name", "ledger_session")
            .unwrap()
            .set_default("cookie_secure", false)
            .unwrap()
            .set_default("rolling_window_secs", 24 * 60 * 60)
            .unwrap()
            .set_default("absolute_secs", 30 * 24 * 60 * 60)
            .unwrap()
            .set_default("idempotency_ttl_secs", 10 * 60)
            .unwrap()
            .add_source(Environment::with_prefix(Self::CONFIG_PREFIX).separator("__"))
            .build()
            .unwrap()
            .try_deserialize::<SessionConfig>()
            .expect("Failed to deserialize Session config")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedConfig {
    pub backfill_size: usize,
    pub broadcast_capacity: usize,
}

impl FeedConfig {
    const CONFIG_PREFIX: &'static str = "FEED_CONFIG";

    pub fn load_from_env() -> Self {
        Config::builder()
            .set_default("backfill_size", 50)
            .unwrap()
            .set_default("broadcast_capacity", 1024)
            .unwrap()
            .add_source(Environment::with_prefix(Self::CONFIG_PREFIX).separator("__"))
            .build()
            .unwrap()
            .try_deserialize::<FeedConfig>()
            .expect("Failed to deserialize Feed config")
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub session: SessionConfig,
    pub feed: FeedConfig,
}

impl AppConfig {
    pub fn load_from_env() -> Self {
        Self {
            server: ServerConfig::load_from_env(),
            session: SessionConfig::load_from_env(),
            feed: FeedConfig::load_from_env(),
        }
    }
}
