use axum::extract::ws::Message;
use serde::Serialize;

use crate::configs::app_state::ChatMessage;

// todo : rename "WsMessageToClient"
#[derive(Debug, Clone)]
pub enum WsMessageToClient {
    Pong,
    JoinLobby(usize),
    LobbiesUpdate(LobbiesGeneralUpdate),
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
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LobbiesGeneralUpdate {
    pub lobbies: Vec<LobbyGeneralUpdate>,
    pub total_users_connected: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct LobbyGeneralUpdate {
    pub player_capacity: usize,
    pub nb_connected: usize,
    pub status: LobbyStatus,
}

#[derive(Debug, Copy, Clone, Serialize, PartialEq, Eq)]
pub enum LobbyStatus {
    AwaitingUsers,
    InGame,
}
