use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

use crate::broadcaster::FeedBroadcaster;
use crate::models::{AuthenticatedUser, Money, TransferCommand, TransferResult};
use crate::repositories::transfers::{TransfersRepository, TransfersRepositoryError};
use crate::repositories::users::{UsersRepository, UsersRepositoryError};

#[derive(Debug, Error)]
pub enum TransfersServiceError {
    #[error("sender user {0} not found")]
    SenderNotFound(i64),

    #[error("recipient '{0}' not found")]
    RecipientNotFound(String),

    #[error("cannot transfer to self")]
    SelfTransfer,

    #[error("currency mismatch")]
    CurrencyMismatch,

    #[error("insufficient funds")]
    InsufficientFunds,

    #[error("system account is not allowed to transact")]
    SystemAccountNotAllowed,

    #[error("ledger invariant violated: {0}")]
    InvariantViolation(String),

    #[error("ledger error: {0}")]
    Ledger(String),

    #[error(transparent)]
    UsersRepository(#[from] UsersRepositoryError),

    #[error(transparent)]
    TransfersRepository(TransfersRepositoryError),
}

impl From<TransfersRepositoryError> for TransfersServiceError {
    fn from(err: TransfersRepositoryError) -> Self {
        use TransfersRepositoryError as R;
        match err {
            R::InvalidAmount => Self::InvariantViolation("amount must be positive".into()),
            R::SelfTransfer => Self::SelfTransfer,
            R::SystemAccountNotAllowed => Self::SystemAccountNotAllowed,
            R::AccountNotFoundSender => Self::SenderNotFound(0),
            R::AccountNotFoundRecipient => Self::RecipientNotFound(String::from("(unknown)")),
            R::CurrencyMismatch => Self::CurrencyMismatch,
            R::InsufficientFunds => Self::InsufficientFunds,
            R::InvariantViolation(msg) => Self::InvariantViolation(msg),
            R::Ledger(msg) => Self::Ledger(msg),
            R::Database(_) => Self::TransfersRepository(err),
        }
    }
}

#[async_trait]
pub trait TransfersService: Send + Sync {
    async fn transfer(&self, cmd: TransferCommand) -> Result<TransferResult, TransfersServiceError>;
}

pub struct TransfersServiceImpl {
    transfers: Arc<dyn TransfersRepository>,
    users: Arc<dyn UsersRepository>,
    broadcaster: Arc<FeedBroadcaster>,
}

impl TransfersServiceImpl {
    pub fn new(
        transfers: Arc<dyn TransfersRepository>,
        users: Arc<dyn UsersRepository>,
        broadcaster: Arc<FeedBroadcaster>,
    ) -> Self {
        Self { transfers, users, broadcaster }
    }
}

#[async_trait]
impl TransfersService for TransfersServiceImpl {
    #[tracing::instrument(
        skip_all,
        fields(
            sender_user_id = cmd.sender_user_id,
            recipient_username = %cmd.recipient_username,
            amount = %cmd.amount.to_decimal_string(),
            currency = %cmd.amount.currency.code,
        )
    )]
    async fn transfer(&self, cmd: TransferCommand) -> Result<TransferResult, TransfersServiceError> {
        let sender: AuthenticatedUser = self
            .users
            .find_by_id(cmd.sender_user_id)
            .await?
            .ok_or(TransfersServiceError::SenderNotFound(cmd.sender_user_id))?
            .into();

        if sender.user.username == cmd.recipient_username {
            return Err(TransfersServiceError::SelfTransfer);
        }

        let recipient = self
            .transfers
            .find_recipient_account(&cmd.recipient_username)
            .await?
            .ok_or_else(|| TransfersServiceError::RecipientNotFound(cmd.recipient_username.clone()))?;

        if recipient.currency_code != cmd.amount.currency.code {
            return Err(TransfersServiceError::CurrencyMismatch);
        }

        let outcome = self
            .transfers
            .execute_transfer(
                sender.account.id,
                recipient.account_id,
                cmd.amount.minor_units,
                &cmd.amount.currency.code,
                cmd.request_id,
                cmd.session_id,
                cmd.sender_user_id,
            )
            .await?;

        let sender_balance = Money::new(outcome.sender_balance_minor_units, cmd.amount.currency.clone());
        let recipient_balance = Money::new(outcome.recipient_balance_minor_units, cmd.amount.currency.clone());

        let result = TransferResult {
            operation_id: outcome.operation_id,
            amount: cmd.amount.clone(),
            sender_balance,
            recipient_balance,
            sender_username: sender.user.username.clone(),
            recipient_username: cmd.recipient_username.clone(),
            created_at: outcome.created_at,
        };

        self.broadcaster.publish((&result).into());

        Ok(result)
    }
}
