use crate::configs::app_state::AppState;
use crate::constants::{MAXIMUM_PLAYERNAME_LENGTH, MINIMUM_PLAYERNAME_LENGTH, PLAYER_NAMES};
use crate::custom_errors::service_errors::ServiceError;
use crate::custom_errors::sqlite_errors::SqliteError;
use crate::data_access_layer::{self, player_dal};
use crate::models::messages_to_clients::WsMessageToClient;
use crate::requests::requests::{
    CreatePlayerRequest, IsValidPlayernameRequest, IsValidPlayernameResponse,
    RequestNewPlayerResponse, UpdateNameRequest,
};
use crate::utilities::responses::{response_ok, ApiResponse};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct Player {
    pub uuid: String,
    pub name: String,
    pub personal_tx: mpsc::UnboundedSender<WsMessageToClient>,
    pub playing_in_lobby: Option<usize>,
    pub queued_moves: VecDeque<PlayerMove>,
    pub xy: (usize, usize),
    pub color: Color,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum PlayerMove {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerMoves {
    pub queued_moves: VecDeque<PlayerMove>,
    pub xy: (usize, usize),
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub enum Color {
    Grey, // Reserved for inactives
    Red,
    Blue,
    Pink,
    Green,
    Yellow,
}

impl Color {
    pub fn pick_available_color(unavailable_colors: &[Color]) -> Option<&Color> {
        const COLORS: [Color; 5] = [
            Color::Red,
            Color::Blue,
            Color::Pink,
            Color::Green,
            Color::Yellow,
        ];
        COLORS
            .iter()
            .find(|&color| !unavailable_colors.contains(color))
    }
}

impl std::str::FromStr for PlayerMove {
    type Err = ();
    fn from_str(input: &str) -> Result<PlayerMove, Self::Err> {
        match input {
            "left" => Ok(PlayerMove::Left),
            "right" => Ok(PlayerMove::Right),
            "up" => Ok(PlayerMove::Up),
            "down" => Ok(PlayerMove::Down),
            _ => Err(()),
        }
    }
}

pub async fn request_new_player(
    State(state): State<Arc<AppState>>,
) -> Result<(StatusCode, Json<ApiResponse<RequestNewPlayerResponse>>), ServiceError> {
    let new_player_name = generate_available_playername(&state)?;
    let new_player_uuid = data_access_layer::player_dal::create_player(
        &state,
        CreatePlayerRequest {
            name: new_player_name.clone(),
        },
    )?;

    response_ok(Some(RequestNewPlayerResponse {
        uuid: new_player_uuid,
        name: new_player_name,
    }))
}

pub async fn get_random_name(
    State(state): State<Arc<AppState>>,
) -> Result<(StatusCode, Json<ApiResponse<String>>), ServiceError> {
    response_ok(Some(generate_available_playername(&state)?))
}

pub async fn is_valid_playername(
    State(state): State<Arc<AppState>>,
    Json(is_valid_playername_request): Json<IsValidPlayernameRequest>,
) -> Result<(StatusCode, Json<ApiResponse<IsValidPlayernameResponse>>), ServiceError> {
    let is_valid = internal_is_valid_playername(is_valid_playername_request.name, &state)?;
    response_ok(Some(is_valid))
}

fn internal_is_valid_playername(
    player_name: String,
    state: &Arc<AppState>,
) -> Result<IsValidPlayernameResponse, ServiceError> {
    // todo : add banword list
    let name_length = player_name.chars().count();
    match name_length.cmp(&MINIMUM_PLAYERNAME_LENGTH) {
        Ordering::Equal => (),
        Ordering::Less => {
            return Ok(IsValidPlayernameResponse {
                is_valid: false,
                reason: Some(format!(
                    "player name is too short ({} characters), it should be at least {}",
                    name_length, MINIMUM_PLAYERNAME_LENGTH
                )),
            })
        }
        Ordering::Greater => match name_length.cmp(&MAXIMUM_PLAYERNAME_LENGTH) {
            Ordering::Greater => {
                return Ok(IsValidPlayernameResponse {
                    is_valid: false,
                    reason: Some(format!(
                        "player name is too long ({} characters), it should be at most {}",
                        name_length, MAXIMUM_PLAYERNAME_LENGTH
                    )),
                })
            }
            _ => (),
        },
    }

    match data_access_layer::player_dal::get_player_by_name(state, player_name.clone()) {
        Ok(_) => Ok(IsValidPlayernameResponse {
            is_valid: false,
            reason: Some("player name already exists".to_string()),
        }),
        Err(_) => Ok(IsValidPlayernameResponse {
            is_valid: true,
            reason: None,
        }),
    }
}

pub async fn set_playername(
    State(state): State<Arc<AppState>>,
    Path(player_uuid): Path<String>,
    Json(update_name_request): Json<UpdateNameRequest>,
) -> Result<(StatusCode, Json<ApiResponse<String>>), ServiceError> {
    // todo : only allow when not playing ?
    let is_valid = internal_is_valid_playername(update_name_request.name.clone(), &state)?;
    if !is_valid.is_valid {
        return Err(ServiceError::PlayerAlreadyExist);
    };
    player_dal::update_playername(
        &state,
        player_uuid.clone(),
        update_name_request.name.clone(),
    )?;

    // todo : check if it's the only thing we need to modify..
    state
        .players
        .write()
        .expect("failed to lock players")
        .entry(player_uuid) // TODO !!! : is it really uuid or name ??
        .and_modify(|e| e.name = update_name_request.name.clone());

    response_ok(Some(update_name_request.name))
}

pub fn generate_available_playername(state: &Arc<AppState>) -> Result<String, ServiceError> {
    let mut rng = rand::thread_rng();

    loop {
        let name = PLAYER_NAMES.choose(&mut rng);
        let full_playername = format!(
            "{}{}{}{}",
            "",
            // "unregistered",
            "#",
            name.expect("failed to pick a random playername"),
            rng.gen_range(0..100000)
        );
        match data_access_layer::player_dal::get_player_by_name(state, full_playername.clone()) {
            Ok(_) => println!(
                "player {} already exists, trying to generate another name",
                full_playername
            ),
            Err(SqliteError::NotFound) => {
                return Ok(full_playername);
            }
            Err(_) => return Err(ServiceError::Internal),
        }
    }
}
