use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::repositories::entities::{FeedActionEntity, RecipientAccountEntity, TransferOutcomeEntity};

#[derive(Debug, Error)]
pub enum TransfersRepositoryError {
    #[error("invalid amount")]
    InvalidAmount,

    #[error("self-transfer not allowed")]
    SelfTransfer,

    #[error("system account is not allowed to transact")]
    SystemAccountNotAllowed,

    #[error("sender account not found")]
    AccountNotFoundSender,

    #[error("recipient account not found")]
    AccountNotFoundRecipient,

    #[error("currency mismatch between accounts")]
    CurrencyMismatch,

    #[error("insufficient funds")]
    InsufficientFunds,

    #[error("ledger invariant violated: {0}")]
    InvariantViolation(String),

    #[error("unknown ledger error: {0}")]
    Ledger(String),

    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for TransfersRepositoryError {
    fn from(err: sqlx::Error) -> Self {
        let sqlx::Error::Database(db) = &err else {
            return Self::Database(err);
        };
        let msg = db.message();
        if msg.contains("INVALID_AMOUNT") {
            Self::InvalidAmount
        } else if msg.contains("SELF_TRANSFER") {
            Self::SelfTransfer
        } else if msg.contains("SYSTEM_ACCOUNT_NOT_ALLOWED") {
            Self::SystemAccountNotAllowed
        } else if msg.contains("ACCOUNT_NOT_FOUND_SENDER") {
            Self::AccountNotFoundSender
        } else if msg.contains("ACCOUNT_NOT_FOUND_RECIPIENT") {
            Self::AccountNotFoundRecipient
        } else if msg.contains("CURRENCY_MISMATCH") {
            Self::CurrencyMismatch
        } else if msg.contains("INSUFFICIENT_FUNDS") {
            Self::InsufficientFunds
        } else if msg.contains("POST_ASSERT") {
            Self::InvariantViolation(msg.to_string())
        } else {
            Self::Database(err)
        }
    }
}

#[async_trait]
pub trait TransfersRepository: Send + Sync {
    async fn execute_transfer(
        &self,
        sender_account_id: i64,
        recipient_account_id: i64,
        amount_minor_units: i64,
        currency: &str,
        request_id: Uuid,
        session_id: Uuid,
        originator_user_id: i64,
    ) -> Result<TransferOutcomeEntity, TransfersRepositoryError>;

    async fn find_recipient_account(
        &self,
        recipient_username: &str,
    ) -> Result<Option<RecipientAccountEntity>, TransfersRepositoryError>;

    async fn list_recent_feed(&self, limit: i64) -> Result<Vec<FeedActionEntity>, TransfersRepositoryError>;
}

pub struct PgTransfersRepository {
    pool: PgPool,
}

impl PgTransfersRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn run_sp_transfer(
        tx: &mut Transaction<'_, Postgres>,
        sender_account_id: i64,
        recipient_account_id: i64,
        amount_minor_units: i64,
        currency: &str,
        request_id: Uuid,
        session_id: Uuid,
        originator_user_id: i64,
    ) -> Result<TransferOutcomeEntity, TransfersRepositoryError> {
        let row = sqlx::query!(
            r#"
            SELECT out_operation_id AS "operation_id!", out_sender_balance AS "sender_balance!", out_recipient_balance AS "recipient_balance!"
            FROM sp_transfer($1, $2, $3, $4::CHAR(3), $5, $6, $7)
            "#,
            sender_account_id,
            recipient_account_id,
            amount_minor_units,
            currency,
            request_id,
            session_id,
            originator_user_id
        )
        .fetch_one(&mut **tx)
        .await?;

        let created_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar!(
            r#"SELECT created_at AS "created_at!" FROM operations WHERE id = $1"#,
            row.operation_id
        )
        .fetch_one(&mut **tx)
        .await?;

        Ok(TransferOutcomeEntity {
            operation_id: row.operation_id,
            sender_balance_minor_units: row.sender_balance,
            recipient_balance_minor_units: row.recipient_balance,
            created_at,
        })
    }
}

#[async_trait]
impl TransfersRepository for PgTransfersRepository {
    #[tracing::instrument(
        skip_all,
        fields(
            sender_account_id = sender_account_id,
            recipient_account_id = recipient_account_id,
            amount_minor_units = amount_minor_units,
            currency = %currency,
        )
    )]
    async fn execute_transfer(
        &self,
        sender_account_id: i64,
        recipient_account_id: i64,
        amount_minor_units: i64,
        currency: &str,
        request_id: Uuid,
        session_id: Uuid,
        originator_user_id: i64,
    ) -> Result<TransferOutcomeEntity, TransfersRepositoryError> {
        let mut tx = self.pool.begin().await?;
        let result = Self::run_sp_transfer(
            &mut tx,
            sender_account_id,
            recipient_account_id,
            amount_minor_units,
            currency,
            request_id,
            session_id,
            originator_user_id,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    #[tracing::instrument(skip_all, fields(recipient_username = %recipient_username))]
    async fn find_recipient_account(
        &self,
        recipient_username: &str,
    ) -> Result<Option<RecipientAccountEntity>, TransfersRepositoryError> {
        let row = sqlx::query!(
            r#"
            SELECT u.id AS user_id, a.id AS account_id, a.currency::TEXT AS "currency!"
            FROM users u JOIN accounts a ON a.user_id = u.id
            WHERE u.username = $1 AND u.is_system = FALSE
            "#,
            recipient_username
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| RecipientAccountEntity {
            user_id: r.user_id,
            account_id: r.account_id,
            currency_code: r.currency,
        }))
    }

    #[tracing::instrument(skip_all, fields(limit = limit))]
    async fn list_recent_feed(&self, limit: i64) -> Result<Vec<FeedActionEntity>, TransfersRepositoryError> {
        let rows = sqlx::query!(
            r#"
            WITH transfer_ops AS (
                SELECT o.id, o.created_at
                FROM operations o
                WHERE o.kind = 'transfer'
                ORDER BY o.created_at DESC, o.id DESC
                LIMIT $1
            )
            SELECT
                op.id AS "operation_id!",
                op.created_at AS "created_at!",
                sender.username AS "sender_username!",
                recipient.username AS "recipient_username!",
                recipient_action.amount AS "amount!",
                recipient_action.currency::TEXT AS "currency!"
            FROM transfer_ops op
            JOIN actions sender_action ON sender_action.operation_id = op.id AND sender_action.amount < 0
            JOIN actions recipient_action ON recipient_action.operation_id = op.id AND recipient_action.amount > 0
            JOIN accounts sender_acc ON sender_acc.id = sender_action.account_id
            JOIN accounts recipient_acc ON recipient_acc.id = recipient_action.account_id
            JOIN users sender ON sender.id = sender_acc.user_id
            JOIN users recipient ON recipient.id = recipient_acc.user_id
            ORDER BY op.created_at DESC, op.id DESC
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| FeedActionEntity {
                operation_id: r.operation_id,
                sender_username: r.sender_username,
                recipient_username: r.recipient_username,
                amount_minor_units: r.amount,
                currency_code: r.currency,
                created_at: r.created_at,
            })
            .collect())
    }
}
