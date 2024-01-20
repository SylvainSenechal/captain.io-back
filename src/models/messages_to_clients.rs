use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Path, State},
    http,
    http::Method,
    response::IntoResponse,
    routing::get,
    routing::post,
    routing::put,
    Router,
};
use serde::Serialize;

use crate::configs::app_state::ChatMessage;

// todo : rename "WsMessageToClient"
#[derive(Debug, Clone)]
pub enum WsMessageToClient {
    Pong,
    JoinLobby(usize),
    LobbiesUpdate(LobbiesUpdate),
    GlobalChatSync(Vec<ChatMessage>),  // get chat history
    GlobalChatNewMessage(ChatMessage), // one new messages
    LobbyChatSync(Vec<ChatMessage>),   // get lobby history
    LobbyChatNewMessage(ChatMessage),  // one new messages
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
                "/lobbiesUpdate ",
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
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LobbiesUpdate {
    pub lobbies: Vec<LobbyUpdate>,
    pub total_users_connected: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct LobbyUpdate {
    pub nb_users: usize,
    pub status: LobbyStatus,
}

#[derive(Debug, Copy, Clone, Serialize)]
pub enum LobbyStatus {
    AwaitingUsers,
    InGame,
}
