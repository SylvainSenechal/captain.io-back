// TODO : important opti ? check rwlock instead of mutex https://doc.rust-lang.org/std/sync/struct.RwLock.html
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::constants::constants::DATABASE_NAME;
use crate::service_layer::player_service::Player;
use crate::{constants::constants, models::messages_to_clients::WsMessageToClient};
#[derive(Debug)]
pub struct AppState {
    pub connection: Pool<SqliteConnectionManager>,
    pub global_broadcast: broadcast::Sender<WsMessageToClient>,
    pub global_chat_messages: Mutex<Vec<ChatMessage>>,
    pub players: Mutex<HashMap<String, Player>>,
    pub lobbies: [Mutex<Lobby>; constants::NB_LOBBIES],
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub message: String,
}

#[derive(Debug)]
pub struct Lobby {
    pub lobby_id: usize,
    pub status: LobbyStatus,
    pub next_starting_time: Option<i64>, // unix timestamp seconds
    pub player_capacity: usize,
    pub lobby_broadcast: broadcast::Sender<WsMessageToClient>,
    pub players: HashSet<String>,
    pub messages: Vec<ChatMessage>,
    pub board_game: Vec<Vec<Tile>>,
}

#[derive(Debug, Copy, Clone, Serialize, PartialEq, Eq)]
pub enum LobbyStatus {
    AwaitingPlayers,
    InGame,
    StartingSoon, // todo : be careful, if a player join and triggers that status and then leaves + what if multiple people leave
}

#[derive(Debug, Clone, Serialize)]
pub struct Tile {
    pub status: TileStatus,
    pub tile_type: TileType,
    pub player_name: Option<String>,
    pub nb_troops: usize, // todo : check which one is best
}
impl Default for Tile {
    fn default() -> Self {
        Tile {
            status: TileStatus::Empty,
            tile_type: TileType::Blank,
            player_name: None,
            nb_troops: 0,
        }
    }
}
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum TileStatus {
    Empty,
    Occupied,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum TileType {
    Blank,
    Kingdom,
    Mountain,
    Castle,
}

impl Lobby {
    fn new(lobby_id: usize, player_capacity: usize) -> Self {
        Self {
            lobby_id: lobby_id,
            status: LobbyStatus::AwaitingPlayers,
            next_starting_time: None,
            player_capacity: player_capacity,
            lobby_broadcast: broadcast::channel(10).0,
            players: HashSet::new(),
            messages: vec![],
            board_game: vec![
                vec![
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                ],
                vec![
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                ],
                vec![
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                ],
                vec![
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                ],
                vec![
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                    Tile::default(),
                ],
            ],
        }
    }
}

impl AppState {
    pub fn new() -> Arc<AppState> {
        let manager = SqliteConnectionManager::file(DATABASE_NAME);
        let pool = r2d2::Pool::builder()
            .max_size(100)
            .build(manager)
            .expect("couldn't create pool");
        let lobbies: [Mutex<Lobby>; constants::NB_LOBBIES] = [
            Mutex::new(Lobby::new(0, 2)),
            Mutex::new(Lobby::new(1, 3)),
            Mutex::new(Lobby::new(2, 1)),
            Mutex::new(Lobby::new(3, 4)),
        ];
        Arc::new(AppState {
            connection: pool,
            global_broadcast: broadcast::channel(100).0,
            global_chat_messages: Mutex::new(vec![]),
            players: Mutex::new(HashMap::new()),
            lobbies: lobbies,
        })
    }
}
