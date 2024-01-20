// TODO : important opti ? check rwlock instead of mutex https://doc.rust-lang.org/std/sync/struct.RwLock.html
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use serde::Serialize;
use tokio::sync::broadcast;

use crate::models;
use crate::service_layer::player_service::Player;
use crate::{
    constants::constants,
    models::messages_to_clients::{LobbyStatus, WsMessageToClient},
};
#[derive(Debug)]
pub struct AppState {
    pub global_broadcast: broadcast::Sender<WsMessageToClient>,
    pub global_chat_messages: Mutex<Vec<ChatMessage>>,
    pub users: Mutex<HashMap<String, Player>>,
    pub lobby: [Mutex<Lobby>; constants::NB_LOBBIES],
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub message: String,
}

#[derive(Debug)]
pub struct Lobby {
    pub lobby_id: usize,
    pub status: LobbyStatus,
    pub lobby_broadcast: broadcast::Sender<WsMessageToClient>,
    pub users: HashSet<String>,
    pub messages: Vec<ChatMessage>,
}

impl Lobby {
    fn new(lobby_id: usize) -> Self {
        Self {
            lobby_id: lobby_id,
            status: LobbyStatus::AwaitingUsers,
            lobby_broadcast: broadcast::channel(10).0,
            users: HashSet::new(),
            messages: vec![ChatMessage {
                message: format!("aLobbyMessage{}", lobby_id),
            }],
        }
    }
}

impl AppState {
    pub fn new() -> Arc<AppState> {
        let lobbies: [Mutex<Lobby>; constants::NB_LOBBIES] = [
            Mutex::new(Lobby::new(0)),
            Mutex::new(Lobby::new(1)),
            Mutex::new(Lobby::new(2)),
        ];
        Arc::new(AppState {
            global_broadcast: broadcast::channel(10).0,
            global_chat_messages: Mutex::new(vec![
                ChatMessage {
                    message: "coucou1".to_string(),
                },
                ChatMessage {
                    message: "coucou2 hehe".to_string(),
                },
                ChatMessage {
                    message: "coucou3".to_string(),
                },
            ]),
            users: Mutex::new(HashMap::new()),
            lobby: lobbies,
        })
    }
}
