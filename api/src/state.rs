use std::sync::Arc;

use crate::broadcaster::FeedBroadcaster;
use crate::clock::Clock;
use crate::config::AppConfig;
use crate::services::auth::AuthService;
use crate::services::currencies::CurrenciesService;
use crate::services::feed::FeedService;
use crate::services::idempotency::IdempotencyService;
use crate::services::transfers::TransfersService;
use crate::services::users::UsersService;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub clock: Arc<dyn Clock>,
    pub broadcaster: Arc<FeedBroadcaster>,
    pub auth: Arc<dyn AuthService>,
    pub users: Arc<dyn UsersService>,
    pub transfers: Arc<dyn TransfersService>,
    pub feed: Arc<dyn FeedService>,
    pub idempotency: Arc<dyn IdempotencyService>,
    pub currencies: Arc<dyn CurrenciesService>,
}
