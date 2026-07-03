use chrono::{DateTime, Duration, Utc};
use std::sync::{Arc, RwLock};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone)]
pub struct TestClock {
    now: Arc<RwLock<DateTime<Utc>>>,
}

impl TestClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: Arc::new(RwLock::new(now)),
        }
    }

    pub fn advance(&self, by: Duration) {
        let mut w = self.now.write().unwrap();
        *w += by;
    }

    pub fn set(&self, when: DateTime<Utc>) {
        *self.now.write().unwrap() = when;
    }
}

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.read().unwrap()
    }
}
