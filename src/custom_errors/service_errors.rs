use crate::custom_errors::sqlite_errors::SqliteError;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiResponseError {
    pub error_message: String,
    pub error_code: ErrorCode,
}

#[derive(Debug)]
pub enum ServiceError {
    Internal,
    PlayerAlreadyExist,
    Sqlite(SqliteError),
    ForbiddenQuery,
    Transaction,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ErrorCode {
    NoError = 0,
    UnspecifiedError = 1,
}

impl ServiceError {
    pub fn error_message(&self) -> String {
        match self {
            Self::Internal => "Internal error".to_string(),
            Self::PlayerAlreadyExist => "Player already exists".to_string(),
            Self::Sqlite(_) => "Sqlite internal error".to_string(),
            Self::ForbiddenQuery => "Query forbidden error".to_string(),
            Self::Transaction => "Transaction error".to_string(),
        }
    }

    fn status_code(&self) -> StatusCode {
        match *self {
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PlayerAlreadyExist => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Sqlite(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ForbiddenQuery => StatusCode::FORBIDDEN,
            Self::Transaction => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let http_status = self.status_code();
        let body = Json(ApiResponseError {
            error_message: self.error_message(),
            error_code: ErrorCode::UnspecifiedError,
        });
        println!("service error encountered : {:?}", self);

        (http_status, body).into_response()
    }
}
