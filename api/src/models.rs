use std::fmt;
use std::str::FromStr;

use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use thiserror::Error;
use uuid::Uuid;

use crate::repositories::entities::{CurrencyEntity, SessionEntity, UserAccountEntity};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Currency {
    pub code: String,
    pub exponent: u8,
}

impl Currency {
    pub fn new(code: impl Into<String>, exponent: u8) -> Self {
        Self {
            code: code.into(),
            exponent,
        }
    }
}

impl From<CurrencyEntity> for Currency {
    fn from(e: CurrencyEntity) -> Self {
        Self {
            code: e.code,
            exponent: e.exponent,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Money {
    pub minor_units: i64,
    pub currency: Currency,
}

#[derive(Debug, Error)]
pub enum MoneyError {
    #[error("invalid decimal format")]
    InvalidFormat,
    #[error("amount must be positive")]
    NotPositive,
    #[error("amount has more decimal places than {0} allows ({1} max)")]
    TooManyDecimalPlaces(String, u8),
    #[error("amount is out of representable range")]
    OutOfRange,
}

impl Money {
    pub fn new(minor_units: i64, currency: Currency) -> Self {
        Self {
            minor_units,
            currency,
        }
    }

    pub fn from_decimal_str(input: &str, currency: Currency) -> Result<Self, MoneyError> {
        let decimal = Decimal::from_str(input.trim()).map_err(|_| MoneyError::InvalidFormat)?;
        if decimal.is_sign_negative() || decimal.is_zero() {
            return Err(MoneyError::NotPositive);
        }
        if decimal.scale() > currency.exponent as u32 {
            return Err(MoneyError::TooManyDecimalPlaces(
                currency.code.clone(),
                currency.exponent,
            ));
        }

        let multiplier = Decimal::from(10i64.pow(currency.exponent as u32));
        let scaled = decimal
            .checked_mul(multiplier)
            .ok_or(MoneyError::OutOfRange)?;
        let minor_units = scaled.to_i64().ok_or(MoneyError::OutOfRange)?;

        Ok(Self {
            minor_units,
            currency,
        })
    }

    pub fn to_decimal_string(&self) -> String {
        let exp = self.currency.exponent as u32;
        let d = Decimal::new(self.minor_units, exp);
        format!("{:.*}", exp as usize, d)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.to_decimal_string(), self.currency.code)
    }
}

#[derive(Clone, Debug)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub is_system: bool,
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: i64,
    pub user_id: i64,
    pub currency: String,
}

#[derive(Clone, Debug)]
pub struct AuthenticatedUser {
    pub user: User,
    pub account: Account,
}

impl From<UserAccountEntity> for AuthenticatedUser {
    fn from(e: UserAccountEntity) -> Self {
        Self {
            user: User {
                id: e.user_id,
                username: e.username,
                email: e.email,
                password_hash: e.password_hash,
                is_system: e.is_system,
            },
            account: Account {
                id: e.account_id,
                user_id: e.user_id,
                currency: e.currency,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: Uuid,
    pub user_id: i64,
    pub created_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub rolling_expires_at: DateTime<Utc>,
    pub absolute_expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl Session {
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && now < self.rolling_expires_at && now < self.absolute_expires_at
    }
}

impl From<SessionEntity> for Session {
    fn from(e: SessionEntity) -> Self {
        Self {
            id: e.id,
            user_id: e.user_id,
            created_at: e.created_at,
            last_activity_at: e.last_activity_at,
            rolling_expires_at: e.rolling_expires_at,
            absolute_expires_at: e.absolute_expires_at,
            revoked_at: e.revoked_at,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TransferRecipient {
    pub username: String,
    pub amount: Money,
}

#[derive(Clone, Debug)]
pub struct TransferCommand {
    pub sender_user_id: i64,
    pub sender_account_id: i64,
    pub recipients: Vec<TransferRecipient>,
    pub currency: Currency,
    pub request_id: Uuid,
    pub session_id: Uuid,
}

#[derive(Clone, Debug)]
pub struct TransferLeg {
    pub action_id: i64,
    pub recipient_username: String,
    pub amount: Money,
}

#[derive(Clone, Debug)]
pub struct TransferResult {
    pub operation_id: Uuid,
    pub sender_username: String,
    pub sender_balance: Money,
    pub currency: Currency,
    pub legs: Vec<TransferLeg>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct FeedEntry {
    pub action_id: i64,
    pub operation_id: Uuid,
    pub sender_username: String,
    pub recipient_username: String,
    pub amount: Money,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct CachedResponse {
    pub status_code: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct IdempotencyRecord {
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub cached: CachedResponse,
    pub expires_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd() -> Currency {
        Currency::new("USD", 2)
    }

    #[test]
    fn parses_clean_decimal() {
        let m = Money::from_decimal_str("12.34", usd()).unwrap();
        assert_eq!(m.minor_units, 1234);
    }

    #[test]
    fn parses_integer_amount() {
        let m = Money::from_decimal_str("12", usd()).unwrap();
        assert_eq!(m.minor_units, 1200);
    }

    #[test]
    fn parses_single_decimal_place() {
        let m = Money::from_decimal_str("12.5", usd()).unwrap();
        assert_eq!(m.minor_units, 1250);
    }

    #[test]
    fn rejects_too_many_decimal_places() {
        let err = Money::from_decimal_str("12.345", usd()).unwrap_err();
        assert!(matches!(err, MoneyError::TooManyDecimalPlaces(_, 2)));
    }

    #[test]
    fn rejects_negative() {
        let err = Money::from_decimal_str("-1.00", usd()).unwrap_err();
        assert!(matches!(err, MoneyError::NotPositive));
    }

    #[test]
    fn rejects_zero() {
        let err = Money::from_decimal_str("0", usd()).unwrap_err();
        assert!(matches!(err, MoneyError::NotPositive));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(
            Money::from_decimal_str("abc", usd()).unwrap_err(),
            MoneyError::InvalidFormat
        ));
    }

    #[test]
    fn formats_with_trailing_zeros() {
        let m = Money {
            minor_units: 1200,
            currency: usd(),
        };
        assert_eq!(m.to_decimal_string(), "12.00");
    }

    #[test]
    fn formats_arbitrary_amount() {
        let m = Money {
            minor_units: 1234567,
            currency: usd(),
        };
        assert_eq!(m.to_decimal_string(), "12345.67");
    }
}
