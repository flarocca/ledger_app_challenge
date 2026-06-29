use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ErrorDetail;

#[derive(Debug, Serialize, ToSchema)]
pub struct Pagination {
    pub next_cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pagination: Option<Pagination>,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorDetail>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(result: T, request_id: Option<Uuid>) -> Self {
        Self {
            result: Some(result),
            pagination: None,
            timestamp: Utc::now(),
            request_id,
            error: None,
        }
    }

    pub fn ok_paginated(result: T, pagination: Pagination, request_id: Option<Uuid>) -> Self {
        Self {
            result: Some(result),
            pagination: Some(pagination),
            timestamp: Utc::now(),
            request_id,
            error: None,
        }
    }

    pub fn error(detail: ErrorDetail, pagination: Option<Pagination>, request_id: Option<Uuid>) -> Self {
        Self {
            result: None,
            pagination,
            timestamp: Utc::now(),
            request_id,
            error: Some(detail),
        }
    }
}
