use std::collections::{HashMap};

use axum::extract::ws::Message;
use serde::Serialize;

use crate::{
    configs::app_state::{ChatMessage, LobbyStatus, Tile},
    service_layer::player_service::{Color, PlayerMoves},
};

#[derive(Debug, Clone)]
pub enum WsMessageToClient {
    Pong,
    JoinLobby(usize),
    LobbiesUpdate(LobbiesGeneralUpdate),
    GlobalChatSync(Vec<ChatMessage>),  // get chat history
    GlobalChatNewMessage(ChatMessage), // one new messages
    LobbyChatSync(Vec<ChatMessage>),   // get lobby history
    LobbyChatNewMessage(ChatMessage),  // one new messages
    GameStarted(usize),                // usize : lobby id
    GameUpdate(GameUpdate),
    WinnerAnnouncement(String),
    QueuedMoves(PlayerMoves),
}

impl WsMessageToClient {
    pub fn to_string_message(&self) -> Message {
        match &self {
            WsMessageToClient::Pong => Message::Text("/pong".to_string()),
            WsMessageToClient::JoinLobby(lobby_id) => {
                Message::Text(format!("/lobbyJoined {}", lobby_id))
            }
            WsMessageToClient::LobbiesUpdate(update) => Message::Text(format!(
                "{}{}",
                "/lobbiesGeneralUpdate ",
                serde_json::to_string(update).expect("failed to jsonize lobbies update")
            )),
            WsMessageToClient::GlobalChatSync(messages) => Message::Text(format!(
                "{}{}",
                "/globalChatSync ",
                serde_json::to_string(messages).expect("failed to jsonize messages")
            )),
            WsMessageToClient::GlobalChatNewMessage(new_message) => Message::Text(format!(
                "{}{}",
                "/globalChatNewMessage ",
                serde_json::to_string(new_message).expect("failed to jsonize new global message")
            )),
            WsMessageToClient::LobbyChatSync(messages) => Message::Text(format!(
                "{}{}",
                "/lobbyChatSync ",
                serde_json::to_string(messages).expect("failed to jsonize messages")
            )),
            WsMessageToClient::LobbyChatNewMessage(new_message) => Message::Text(format!(
                "{}{}",
                "/lobbyChatNewMessage ",
                serde_json::to_string(new_message).expect("failed to jsonize message")
            )),
            WsMessageToClient::GameStarted(lobby_id) => {
                Message::Text(format!("/gameStarted {}", lobby_id))
            }
            WsMessageToClient::GameUpdate(game_state) => Message::Text(format!(
                "{}{}",
                "/gameUpdate ",
                serde_json::to_string(game_state).expect("failed to jsonize game_state")
            )),
            WsMessageToClient::WinnerAnnouncement(winner_name) => {
                Message::Text(format!("{}{}", "/winnerIs ", winner_name))
            }
            WsMessageToClient::QueuedMoves(moves) => Message::Text(format!(
                "{}{}",
                "/myMoves ",
                serde_json::to_string(moves).expect("failed to jsonize game_state")
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LobbiesGeneralUpdate {
    pub lobbies: Vec<LobbyGeneralUpdate>,
    pub total_players_connected: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct LobbyGeneralUpdate {
    pub player_capacity: usize,
    pub nb_connected: usize,
    pub status: LobbyStatus,
    pub next_starting_time: Option<i64>, // unix timestamp seconds
}

#[derive(Debug, Clone, Serialize)]
pub struct GameUpdate {
    pub board_game: Vec<Vec<Tile>>,
    pub score_board: HashMap<String, PlayerScore>,
    pub moves: PlayerMoves,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerScore {
    pub total_troops: usize,
    pub total_positions: usize,
    pub color: Color,
}
