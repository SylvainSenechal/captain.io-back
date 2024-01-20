use crate::configs::app_state::AppState;
use crate::errors::service_errors::ServiceError;
use crate::models::messages_to_clients::WsMessageToClient;
use crate::utilities::responses::{response_ok, response_ok_with_message, ApiResponse};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug)]
// pub struct Player {}
pub struct Player {
    // todo : change to different folder ?
    pub uuid: String,
    pub name: Option<String>,
    pub personal_tx: broadcast::Sender<WsMessageToClient>,
    pub playing_in_lobby: Option<usize>,
}

pub async fn request_uuid(
    State(state): State<Arc<AppState>>,
) -> Result<(StatusCode, Json<ApiResponse<String>>), ServiceError> {
    let player_uuid = Uuid::now_v7().to_string();

    response_ok(Some(player_uuid))
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateUsernameRequest {
    username: String,
}

pub async fn set_username(
    State(state): State<Arc<AppState>>,
    Path(user_uuid): Path<String>,
    // Json(mut create_user_request): Json<requests::CreateUserRequest>,
    Json(update_username_request): Json<UpdateUsernameRequest>,
) -> Result<(StatusCode, Json<ApiResponse<()>>), ServiceError> {
    // todo : auth for this ?
    state
        .users
        .lock()
        .expect("failed to lock users")
        .entry(user_uuid)
        .and_modify(|e| e.name = Some(update_username_request.username));

    response_ok(None::<()>)
}
