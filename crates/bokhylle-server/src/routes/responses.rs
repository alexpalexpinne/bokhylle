//! Small, typed envelopes shared by HTTP handlers.

use aide::generate::GenContext;
use aide::openapi::{Operation, Response};
use aide::operation::OperationOutput;
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response as AxumResponse};
use schemars::JsonSchema;
use serde::Serialize;

/// A JSON body whose success status is part of its OpenAPI return type.
pub struct StatusJson<T, const STATUS: u16>(pub T);

impl<T: Serialize, const STATUS: u16> IntoResponse for StatusJson<T, STATUS> {
    fn into_response(self) -> AxumResponse {
        (
            StatusCode::from_u16(STATUS).expect("valid API status"),
            Json(self.0),
        )
            .into_response()
    }
}

impl<T: JsonSchema, const STATUS: u16> OperationOutput for StatusJson<T, STATUS> {
    type Inner = T;

    fn operation_response(ctx: &mut GenContext, operation: &mut Operation) -> Option<Response> {
        <Json<T> as OperationOutput>::operation_response(ctx, operation)
    }

    fn inferred_responses(
        ctx: &mut GenContext,
        operation: &mut Operation,
    ) -> Vec<(Option<u16>, Response)> {
        Self::operation_response(ctx, operation)
            .map(|response| vec![(Some(STATUS), response)])
            .unwrap_or_default()
    }
}

/// Request creation returns 201 for new work and 200 for an existing request.
pub struct CreatedOrOk<T> {
    pub value: T,
    pub created: bool,
}

impl<T: Serialize> IntoResponse for CreatedOrOk<T> {
    fn into_response(self) -> AxumResponse {
        let status = if self.created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };
        (status, Json(self.value)).into_response()
    }
}

impl<T: JsonSchema> OperationOutput for CreatedOrOk<T> {
    type Inner = T;

    fn operation_response(ctx: &mut GenContext, operation: &mut Operation) -> Option<Response> {
        <Json<T> as OperationOutput>::operation_response(ctx, operation)
    }

    fn inferred_responses(
        ctx: &mut GenContext,
        operation: &mut Operation,
    ) -> Vec<(Option<u16>, Response)> {
        Self::operation_response(ctx, operation)
            .map(|response| vec![(Some(200), response.clone()), (Some(201), response)])
            .unwrap_or_default()
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct Items<T> {
    pub items: Vec<T>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct Tokens<T> {
    pub tokens: Vec<T>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct CreatedToken {
    pub id: i64,
    pub token: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct UpdatedCount {
    pub updated: u64,
}
