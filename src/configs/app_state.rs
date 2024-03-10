use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rand::Rng;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::service_layer::player_service::Player;
use crate::{constants, models::messages_to_clients::WsMessageToClient};
#[derive(Debug)]
pub struct AppState {
    pub connection: Pool<SqliteConnectionManager>,
    pub global_broadcast: broadcast::Sender<WsMessageToClient>,
    pub global_chat_messages: RwLock<Vec<ChatMessage>>,
    pub players: RwLock<HashMap<String, Player>>,
    pub lobbies: [RwLock<Lobby>; constants::NB_LOBBIES],
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
    pub players: HashMap<String, String>, // name->uuid
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
    pub nb_troops: usize,
    pub hidden: bool,
}
impl Default for Tile {
    fn default() -> Self {
        Tile {
            status: TileStatus::Empty,
            tile_type: TileType::Blank,
            player_name: None,
            nb_troops: 0,
            hidden: true, // todo : has no business being stored here, should only be in returned board, not backend board
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
        let mut lobby = Lobby {
            lobby_id,
            status: LobbyStatus::AwaitingPlayers,
            next_starting_time: None,
            player_capacity,
            lobby_broadcast: broadcast::channel(10).0,
            players: HashMap::new(),
            messages: vec![],
            board_game: vec![],
        };
        lobby.generate_new_board();
        lobby
    }
    pub fn generate_new_board(&mut self) {
        let mut rng = rand::thread_rng();
        let width = rng.gen_range(5..8);
        let height = rng.gen_range(15..20);
        self.board_game = vec![];
        for _ in 0..width {
            let mut column = vec![];
            for _ in 0..height {
                column.push(Tile::default())
            }
            self.board_game.push(column)
        }
        for _ in 0..10 {
            let x = rng.gen_range(0..width);
            let y = rng.gen_range(0..height);
            if self.board_game[x][y].tile_type == TileType::Blank {
                self.board_game[x][y].tile_type = TileType::Mountain;
            }
        }
        for _ in 0..10 {
            let x = rng.gen_range(0..width);
            let y = rng.gen_range(0..height);
            if self.board_game[x][y].tile_type == TileType::Blank {
                self.board_game[x][y].tile_type = TileType::Castle;
                self.board_game[x][y].nb_troops = 15;
            }
        }
    }
}

impl AppState {
    pub fn new() -> Arc<AppState> {
        let manager = SqliteConnectionManager::file(constants::DATABASE_NAME);
        let pool = r2d2::Pool::builder()
            .max_size(100)
            .build(manager)
            .expect("couldn't create pool");
        let lobbies: [RwLock<Lobby>; constants::NB_LOBBIES] = [
            RwLock::new(Lobby::new(0, 2)),
            RwLock::new(Lobby::new(1, 3)),
            RwLock::new(Lobby::new(2, 1)),
            RwLock::new(Lobby::new(3, 4)),
        ];
        Arc::new(AppState {
            connection: pool,
            global_broadcast: broadcast::channel(100).0,
            global_chat_messages: RwLock::new(vec![]),
            players: RwLock::new(HashMap::new()),
            lobbies,
        })
    }
}
