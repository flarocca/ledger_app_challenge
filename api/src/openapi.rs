use utoipa::OpenApi;

use crate::broadcaster::FeedEvent;
use crate::error::ErrorDetail;
use crate::handlers::auth::requests::LoginRequest;
use crate::handlers::auth::responses::{LoginResponse, LogoutResponse};
use crate::handlers::feed::responses::{FeedItem, FeedListResponse};
use crate::handlers::transfers::requests::{CreateTransferRequest, TransferRecipientRequest};
use crate::handlers::transfers::responses::{TransferLegResponse, TransferResponse};
use crate::handlers::users::responses::MeResponse;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Ledger API",
        description = "Money transfer and global feed API",
        version = "0.1.0"
    ),
    paths(
        crate::handlers::auth::post::login,
        crate::handlers::auth::post::logout,
        crate::handlers::users::get::me,
        crate::handlers::transfers::post::create,
        crate::handlers::feed::get::list,
        crate::handlers::feed::stream::stream,
    ),
    components(schemas(
        ErrorDetail,
        LoginRequest,
        LoginResponse,
        LogoutResponse,
        MeResponse,
        CreateTransferRequest,
        TransferRecipientRequest,
        TransferResponse,
        TransferLegResponse,
        FeedItem,
        FeedListResponse,
        FeedEvent,
    )),
    tags(
        (name = "auth", description = "Authentication"),
        (name = "users", description = "User lookup"),
        (name = "transfers", description = "Money transfers"),
        (name = "feed", description = "Global feed"),
    )
)]
pub struct ApiDoc;
