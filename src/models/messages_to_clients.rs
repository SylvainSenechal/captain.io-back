use std::{any::Any, collections::HashMap, sync::Arc};

use axum::extract::ws::Message;
use serde::Serialize;

use crate::{
    configs::{
        self,
        app_state::{ChatMessage, LobbyStatus, Tile, TileStatus, TileType},
    },
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
    pub connected_players: Vec<(String, Option<usize>)>, // (name,in lobby)
}
#[derive(Debug, Clone, Serialize)]
pub struct LobbyGeneralUpdate {
    pub player_capacity: usize,
    pub player_names: Vec<String>,
    pub status: LobbyStatus,
    pub next_starting_time: i64, // unix timestamp seconds
}

#[derive(Debug, Clone, Serialize)]
pub struct GameUpdate {
    pub board_game: Vec<Vec<TileUpdate>>,
    pub score_board: HashMap<String, PlayerScore>,
    pub moves: PlayerMoves,
    pub tick: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct TileUpdate {
    pub status: TileStatus,
    pub tile_type: TileType,
    pub player_name: Option<String>,
    pub nb_troops: usize,
    pub hidden: bool,
}

impl TileUpdate {
    pub fn from_game_tile(&mut self, tile: &Tile, lobby_players: &HashMap<String, String>) {
        self.status = tile.status.clone();
        self.tile_type = tile.tile_type.clone();
        self.player_name = None;
        self.nb_troops = tile.nb_troops;
        self.hidden = false;
        if let Some(player_uuid) = tile.player_uuid.clone() {
            self.player_name = Some(
                lobby_players
                    .get(&player_uuid)
                    .expect("couldn't find player")
                    .into(),
            );
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerScore {
    pub total_troops: usize,
    pub total_positions: usize,
    pub color: Color,
}
