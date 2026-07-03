use crate::error::ApiError;
use crate::handlers::transfers::requests::CreateTransferRequest;
use crate::handlers::transfers::responses::TransferResponse;
use crate::middlewares::authentication::AuthContext;
use crate::middlewares::correlation_id::RequestId;
use crate::models::{Money, TransferCommand, TransferRecipient};
use crate::response::ApiResponse;
use crate::state::AppState;
use axum::Extension;
use axum::extract::State;
use axum::response::IntoResponse;
use validator::Validate;

#[utoipa::path(
    post,
    path = "/transfers",
    tag = "transfers",
    request_body = CreateTransferRequest,
    responses(
        (status = 200, description = "Transfer recorded", body = TransferResponse),
        (status = 400, description = "Malformed request (validation, decimal parse)"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Recipient not found"),
        (status = 422, description = "Business constraint violated (insufficient funds, self-transfer, duplicate recipient, currency mismatch, unsupported currency)"),
    )
)]
#[tracing::instrument(
    skip_all,
    fields(
        user_id = auth.user.user.id,
        recipient_count = req.recipients.len(),
        currency = %req.currency,
    )
)]
pub async fn create(
    State(state): State<AppState>,
    Extension(RequestId(request_id)): Extension<RequestId>,
    Extension(auth): Extension<AuthContext>,
    axum::Json(req): axum::Json<CreateTransferRequest>,
) -> Result<impl IntoResponse, ApiError> {
    req.validate()?;

    let currency = state.currencies.require(&req.currency).await?;

    let mut recipients = Vec::with_capacity(req.recipients.len());
    for entry in req.recipients {
        let amount = Money::from_decimal_str(&entry.amount, currency.clone())?;
        recipients.push(TransferRecipient {
            username: entry.recipient_username,
            amount,
        });
    }

    let cmd = TransferCommand {
        sender_user_id: auth.user.user.id,
        sender_account_id: auth.user.account.id,
        recipients,
        currency,
        request_id,
        session_id: auth.session.id,
    };

    let result = state.transfers.transfer(cmd).await?;
    let body = TransferResponse::from(result);
    Ok(axum::Json(ApiResponse::ok(body, Some(request_id))))
}
