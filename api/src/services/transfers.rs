use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

use crate::broadcaster::{FeedBroadcaster, FeedEvent};
use crate::models::{
    AuthenticatedUser, Money, TransferCommand, TransferLeg, TransferRecipient, TransferResult,
};
use crate::repositories::entities::TransferOutcomeEntity;
use crate::repositories::transfers::{TransfersRepository, TransfersRepositoryError};
use crate::repositories::users::{UsersRepository, UsersRepositoryError};

#[derive(Debug, Error)]
pub enum TransfersServiceError {
    #[error("sender user {0} not found")]
    SenderNotFound(i64),

    #[error("recipient '{0}' not found")]
    RecipientNotFound(String),

    #[error("at least one recipient is required")]
    NoRecipients,

    #[error("recipient '{0}' appears more than once in the same transfer")]
    DuplicateRecipient(String),

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
            R::DuplicateRecipient => Self::DuplicateRecipient("(unknown)".into()),
            R::NoRecipients => Self::NoRecipients,
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
    async fn transfer(&self, cmd: TransferCommand)
    -> Result<TransferResult, TransfersServiceError>;
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
        Self {
            transfers,
            users,
            broadcaster,
        }
    }
}

struct ResolvedRecipient {
    username: String,
    account_id: i64,
    amount: Money,
}

impl TransfersServiceImpl {
    async fn load_sender(
        &self,
        sender_user_id: i64,
    ) -> Result<AuthenticatedUser, TransfersServiceError> {
        self.users
            .find_by_id(sender_user_id)
            .await?
            .ok_or(TransfersServiceError::SenderNotFound(sender_user_id))
            .map(Into::into)
    }

    fn validate_recipients(
        sender: &AuthenticatedUser,
        recipients: &[TransferRecipient],
        currency_code: &str,
    ) -> Result<(), TransfersServiceError> {
        let mut seen = HashSet::with_capacity(recipients.len());
        for r in recipients {
            if r.username == sender.user.username {
                return Err(TransfersServiceError::SelfTransfer);
            }
            if !seen.insert(r.username.as_str()) {
                return Err(TransfersServiceError::DuplicateRecipient(
                    r.username.clone(),
                ));
            }
            if r.amount.currency.code != currency_code {
                return Err(TransfersServiceError::CurrencyMismatch);
            }
        }
        Ok(())
    }

    async fn resolve_recipients(
        &self,
        recipients: Vec<TransferRecipient>,
        currency_code: &str,
    ) -> Result<Vec<ResolvedRecipient>, TransfersServiceError> {
        let mut resolved = Vec::with_capacity(recipients.len());
        for TransferRecipient { username, amount } in recipients {
            let account = self
                .transfers
                .find_recipient_account(&username)
                .await?
                .ok_or_else(|| TransfersServiceError::RecipientNotFound(username.clone()))?;
            if account.currency_code != currency_code {
                return Err(TransfersServiceError::CurrencyMismatch);
            }
            resolved.push(ResolvedRecipient {
                username,
                account_id: account.account_id,
                amount,
            });
        }
        Ok(resolved)
    }

    async fn execute_ledger(
        &self,
        sender: &AuthenticatedUser,
        resolved: &[ResolvedRecipient],
        currency_code: &str,
        request_id: uuid::Uuid,
        session_id: uuid::Uuid,
        sender_user_id: i64,
    ) -> Result<TransferOutcomeEntity, TransfersServiceError> {
        let recipient_account_ids: Vec<i64> = resolved.iter().map(|r| r.account_id).collect();
        let amounts: Vec<i64> = resolved.iter().map(|r| r.amount.minor_units).collect();
        let outcome = self
            .transfers
            .execute_transfer(
                sender.account.id,
                &recipient_account_ids,
                &amounts,
                currency_code,
                request_id,
                session_id,
                sender_user_id,
            )
            .await?;
        Ok(outcome)
    }

    fn assemble_legs(
        resolved: Vec<ResolvedRecipient>,
        outcome: &TransferOutcomeEntity,
    ) -> Result<Vec<TransferLeg>, TransfersServiceError> {
        resolved
            .into_iter()
            .map(|r| {
                let action_id = outcome
                    .legs
                    .iter()
                    .find(|leg| leg.recipient_account_id == r.account_id)
                    .map(|leg| leg.action_id)
                    .ok_or_else(|| {
                        TransfersServiceError::Ledger(format!(
                            "missing action id for {}",
                            r.username
                        ))
                    })?;
                Ok(TransferLeg {
                    action_id,
                    recipient_username: r.username,
                    amount: r.amount,
                })
            })
            .collect()
    }

    fn broadcast_legs(&self, result: &TransferResult) {
        for leg in &result.legs {
            self.broadcaster.publish(FeedEvent {
                id: leg.action_id,
                operation_id: result.operation_id,
                sender_username: result.sender_username.clone(),
                recipient_username: leg.recipient_username.clone(),
                amount: leg.amount.to_decimal_string(),
                currency: result.currency.code.clone(),
                created_at: result.created_at,
            });
        }
    }
}

#[async_trait]
impl TransfersService for TransfersServiceImpl {
    #[tracing::instrument(
        skip_all,
        fields(
            sender_user_id = cmd.sender_user_id,
            recipient_count = cmd.recipients.len(),
            currency = %cmd.currency.code,
        )
    )]
    async fn transfer(
        &self,
        cmd: TransferCommand,
    ) -> Result<TransferResult, TransfersServiceError> {
        let TransferCommand {
            sender_user_id,
            sender_account_id: _,
            recipients,
            currency,
            request_id,
            session_id,
        } = cmd;

        if recipients.is_empty() {
            return Err(TransfersServiceError::NoRecipients);
        }

        let sender = self.load_sender(sender_user_id).await?;
        Self::validate_recipients(&sender, &recipients, &currency.code)?;

        let resolved = self.resolve_recipients(recipients, &currency.code).await?;
        let outcome = self
            .execute_ledger(
                &sender,
                &resolved,
                &currency.code,
                request_id,
                session_id,
                sender_user_id,
            )
            .await?;
        let legs = Self::assemble_legs(resolved, &outcome)?;

        let result = TransferResult {
            operation_id: outcome.operation_id,
            sender_username: sender.user.username,
            sender_balance: Money::new(outcome.sender_balance_minor_units, currency.clone()),
            currency,
            legs,
            created_at: outcome.created_at,
        };

        self.broadcast_legs(&result);

        Ok(result)
    }
}
