use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use tokio::sync::broadcast;

use crate::constants::constants;
use crate::{service_layer::player_service::Player, ClientMessage};
// todo : check rwlock instead of mutex
#[derive(Debug)]
pub struct AppState {
    pub global_broadcast: broadcast::Sender<ClientMessage>,
    pub users: Mutex<HashMap<String, Player>>,
    pub lobby: [Mutex<Lobby>; constants::NB_LOBBIES],
}

#[derive(Debug)]
pub struct Lobby {
    pub lobby_id: usize,
    pub lobby_broadcast: broadcast::Sender<ClientMessage>,
    pub users: HashSet<String>,
}

impl Lobby {
    fn new(lobby_id: usize) -> Self {
        Self {
            lobby_id: lobby_id,
            lobby_broadcast: broadcast::channel(100).0,
            users: HashSet::new(),
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
            global_broadcast: broadcast::channel(100).0,
            users: Mutex::new(HashMap::new()),
            lobby: lobbies,
        })
    }
}
