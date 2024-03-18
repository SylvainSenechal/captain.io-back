use crate::{
    configs::{
        self,
        app_state::{Lobby, LobbyStatus, Tile, TileStatus, TileType},
    },
    constants::{TICK_BLANK, TICK_CASTLE, TICK_GAME_INTERVAL_MS, TICK_KINGDOM},
    models::messages_to_clients::{GameUpdate, PlayerScore, TileUpdate, WsMessageToClient},
};
use chrono::Utc;
use rand::Rng;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};
use tokio::time::{interval, Duration};

use super::{
    player_service::{Color, PlayerMove, PlayerMoves},
    websocket_service::global_lobbies_update,
};

pub async fn game_loop(state: Arc<configs::app_state::AppState>) {
    let mut interval = interval(Duration::from_millis(TICK_GAME_INTERVAL_MS));
    loop {
        interval.tick().await; // The first tick completes immediately
        for mutex_lobby in state.lobbies.iter() {
            let mut lobby = mutex_lobby.write().expect("failed to lock lobby");
            match lobby.status {
                LobbyStatus::AwaitingPlayers => (),
                LobbyStatus::StartingSoon => {
                    if lunch_game(state.clone(), &mut lobby) {
                        drop(lobby); // global lobbies update needs to take ownership of all the lobbies
                        global_lobbies_update(state.clone());
                    }
                }
                LobbyStatus::InGame => {
                    if let Ok(is_game_finished) = tick_game(&mut lobby, state.clone()) {
                        if is_game_finished {
                            end_lobby_game(&mut lobby, state.clone());
                            drop(lobby);
                            global_lobbies_update(state.clone());
                        }
                    }
                }
            };
        }
    }
}

fn lunch_game(state: Arc<configs::app_state::AppState>, lobby: &mut Lobby) -> bool {
    if lobby.next_starting_time - Utc::now().timestamp() <= 0 {
        lobby.status = LobbyStatus::InGame;
        let mut unavailable_colors = vec![];
        let mut players = state.players.write().expect("failed to lock player");
        for (player_uuid, _player_name) in lobby.players.iter() {
            let new_player_color = Color::pick_available_color(&unavailable_colors)
                .expect("no player color available");
            let (x, y) = pick_available_starting_coordinates(&lobby.board_game);
            if let Some(player) = players.get_mut(&player_uuid.clone()) {
                // the player could leave the lobby while the game is lunching..
                player.queued_moves = VecDeque::new();
                player.color = new_player_color.clone();
                unavailable_colors.push(new_player_color.clone());
                player.xy = (x, y);
            }
            lobby.board_game[x][y] = Tile {
                // still add him in the map, will be displayed as inactive
                status: TileStatus::Occupied,
                tile_type: TileType::Kingdom,
                nb_troops: 1,
                player_uuid: Some(player_uuid.clone()),
            };
        }
        let _ = lobby
            .lobby_broadcast
            .send(WsMessageToClient::GameStarted(lobby.lobby_id));

        true
    } else {
        false
    }
}

fn tick_game(lobby: &mut Lobby, state: Arc<configs::app_state::AppState>) -> Result<bool, String> {
    lobby.tick += 1;
    for position in lobby.board_game.iter_mut().flatten() {
        match position.status {
            TileStatus::Occupied => match position.tile_type {
                TileType::Kingdom if lobby.tick % TICK_KINGDOM == 0 => position.nb_troops += 1,
                TileType::Castle if lobby.tick % TICK_CASTLE == 0 => position.nb_troops += 1,
                TileType::Blank if lobby.tick % TICK_BLANK == 0 => position.nb_troops += 1,
                TileType::Mountain => (),
                _ => (),
            },
            TileStatus::Empty => (),
        }
    }
    let mut players = state.players.write().expect("failed to lock players");
    let mut scoreboard: HashMap<String, PlayerScore> = HashMap::new();
    let width_game = lobby.board_game.len();
    let height_game = lobby.board_game[0].len();

    'loop_attackers: for (player_uuid, player_name) in lobby.players.iter() {
        scoreboard.insert(
            player_name.clone(),
            PlayerScore {
                total_positions: 0,
                total_troops: 0,
                color: Color::Grey,
            },
        );
        if let Some(attacker) = players.get_mut(player_uuid) {
            match attacker.playing_in_lobby {
                None => continue 'loop_attackers, // if the player left the webpage
                Some(lobby_id) => {
                    if lobby_id != lobby.lobby_id {
                        // The player refreshed and went inside another game
                        continue 'loop_attackers;
                    }
                }
            };
            scoreboard
                .get_mut(&attacker.name.clone())
                .expect("no attacker name in score board")
                .color = attacker.color.clone();

            if let Some(next_move) = attacker.queued_moves.pop_front() {
                let mut attacked_x = attacker.xy.0;
                let mut attacked_y = attacker.xy.1;
                match next_move {
                    PlayerMove::Left => {
                        attacked_x = attacker.xy.0.saturating_sub(1);
                    }
                    PlayerMove::Right => {
                        attacked_x = (attacker.xy.0 + 1).min(width_game - 1);
                    }
                    PlayerMove::Up => {
                        attacked_y = attacker.xy.1.saturating_sub(1);
                    }
                    PlayerMove::Down => {
                        attacked_y = (attacker.xy.1 + 1).min(height_game - 1);
                    }
                }
                match resolve_assault(
                    attacker.uuid.clone(),
                    &lobby.board_game,
                    attacker.xy,
                    (attacked_x, attacked_y),
                ) {
                    OutcomeAssault::AttackingSameTile => (),
                    OutcomeAssault::BlockedByMountain => (),
                    OutcomeAssault::NotEnoughTroops => (),
                    OutcomeAssault::TileNotOwned => (),
                    OutcomeAssault::SelfTroopsMove => {
                        lobby.board_game[attacked_x][attacked_y].nb_troops +=
                            lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops - 1;
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                    }
                    OutcomeAssault::ConquerEmpty => {
                        lobby.board_game[attacked_x][attacked_y] = Tile {
                            status: TileStatus::Occupied,
                            tile_type: lobby.board_game[attacked_x][attacked_y].tile_type.clone(),
                            player_uuid: Some(attacker.uuid.clone()),
                            nb_troops: lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops - 1,
                        };
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                    }
                    OutcomeAssault::Tie => {
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                        lobby.board_game[attacked_x][attacked_y].nb_troops = 0;
                    }
                    OutcomeAssault::Victory(loser_uuid, nb_remaining) => {
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                        lobby.board_game[attacked_x][attacked_y] = Tile {
                            status: TileStatus::Occupied,
                            tile_type: lobby.board_game[attacked_x][attacked_y].tile_type.clone(),
                            player_uuid: Some(attacker.uuid.clone()),
                            nb_troops: nb_remaining,
                        };
                        if lobby.board_game[attacked_x][attacked_y].tile_type == TileType::Kingdom {
                            lobby.board_game[attacked_x][attacked_y].tile_type = TileType::Castle;
                            for position in lobby.board_game.iter_mut().flatten() {
                                if let Some(occupier_uuid) = position.player_uuid.clone() {
                                    if occupier_uuid == loser_uuid {
                                        position.player_uuid = Some(attacker.uuid.clone());
                                    }
                                }
                            }
                        }
                    }
                    OutcomeAssault::Defeat(defensive_losses) => {
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                        lobby.board_game[attacked_x][attacked_y].nb_troops = defensive_losses;
                    }
                    OutcomeAssault::VictoryCastle(nb_remaining) => {
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                        lobby.board_game[attacked_x][attacked_y] = Tile {
                            status: TileStatus::Occupied,
                            tile_type: lobby.board_game[attacked_x][attacked_y].tile_type.clone(),
                            player_uuid: Some(attacker.uuid.clone()),
                            nb_troops: nb_remaining,
                        };
                    }
                }
                attacker.xy = (attacked_x, attacked_y);

                println!("next move: {:?}", next_move);
            }
        }
    }

    let mut remaining_players = HashSet::new();
    for position in lobby.board_game.iter().flatten() {
        if let Some(occupier_uuid) = position.player_uuid.clone() {
            let player_name = lobby
                .players
                .get(&occupier_uuid)
                .expect("couldn't find player");
            remaining_players.insert(player_name);
            scoreboard.entry(player_name.clone()).and_modify(|score| {
                score.total_positions += 1;
                score.total_troops += position.nb_troops;
            });
        }
    }

    let mut nb_active = 0; // todo : count differently with filter
    for (_, score) in scoreboard.iter() {
        if score.color != Color::Grey {
            nb_active += 1;
        }
    }
    for (_, player) in players.iter() {
        let mut personal_board_game: Vec<Vec<TileUpdate>> = vec![];
        let width = lobby.board_game.len();
        let height = lobby.board_game[0].len();
        for i in 0..width {
            let mut column = vec![];
            for j in 0..height {
                let hidden_type = match lobby.board_game[i][j].tile_type {
                    TileType::Blank => TileType::Blank,
                    TileType::Kingdom => TileType::Mountain,
                    TileType::Castle => TileType::Mountain,
                    TileType::Mountain => TileType::Mountain,
                };
                column.push(TileUpdate {
                    status: TileStatus::Empty,
                    tile_type: hidden_type,
                    player_name: None,
                    nb_troops: 0,
                    hidden: true,
                })
            }
            personal_board_game.push(column);
        }
        for i in 0..width {
            for j in 0..height {
                if let Some(uuid) = lobby.board_game[i][j].player_uuid.clone() {
                    if uuid == player.uuid {
                        let min_w = i.saturating_sub(1);
                        let min_h = j.saturating_sub(1);
                        let max_w = (i + 1).min(width - 1);
                        let max_h = (j + 1).min(height - 1);

                        personal_board_game[min_w][min_h]
                            .from_game_tile(&lobby.board_game[min_w][min_h], &lobby.players);
                        personal_board_game[min_w][j]
                            .from_game_tile(&lobby.board_game[min_w][j], &lobby.players);
                        personal_board_game[min_w][max_h]
                            .from_game_tile(&lobby.board_game[min_w][max_h], &lobby.players);
                        personal_board_game[i][min_h]
                            .from_game_tile(&lobby.board_game[i][min_h], &lobby.players);
                        personal_board_game[i][j]
                            .from_game_tile(&lobby.board_game[i][j], &lobby.players);
                        personal_board_game[i][max_h]
                            .from_game_tile(&lobby.board_game[i][max_h], &lobby.players);
                        personal_board_game[max_w][min_h]
                            .from_game_tile(&lobby.board_game[max_w][min_h], &lobby.players);
                        personal_board_game[max_w][j]
                            .from_game_tile(&lobby.board_game[max_w][j], &lobby.players);
                        personal_board_game[max_w][max_h]
                            .from_game_tile(&lobby.board_game[max_w][max_h], &lobby.players);
                    }
                }
            }
        }

        let _ = player
            .personal_tx
            .send(WsMessageToClient::GameUpdate(GameUpdate {
                board_game: personal_board_game,
                score_board: scoreboard.clone(),
                moves: PlayerMoves {
                    queued_moves: player.queued_moves.clone(),
                    xy: player.xy,
                },
                tick: lobby.tick,
            }));
    }

    println!("aaaaa 1 {} ", nb_active);
    println!("aaaaa 2 {} ", remaining_players.len());
    match remaining_players.len() {
        1 => {
            let _ = lobby
                .lobby_broadcast
                .send(WsMessageToClient::WinnerAnnouncement(
                    remaining_players
                        .iter()
                        .next()
                        .expect("no remaining player to win")
                        .to_string(),
                ));
            Ok(true)
        }
        0 => {
            let _ = lobby
                .lobby_broadcast
                .send(WsMessageToClient::WinnerAnnouncement("".to_string())); // todo : handle with none
            Ok(true)
        }
        _ if nb_active == 0 => Ok(true), // player still occupying some tiles, but nobody is active
        _ => Ok(false),
    }
}

pub fn end_lobby_game(lobby: &mut Lobby, state: Arc<configs::app_state::AppState>) {
    let mut all_players = state
        .players
        .write()
        .expect("failed to lock players end game");

    for (player_uuid, _) in lobby.players.iter() {
        for (_, player) in all_players.iter_mut() {
            if player_uuid.clone() == player.uuid {
                if let Some(already_in_lobby_id) = player.playing_in_lobby {
                    if already_in_lobby_id == lobby.lobby_id {
                        // only reset if not already started another game (after leaving the current one)
                        player.playing_in_lobby = None;
                    }
                }
            }
        }
    }
    lobby.generate_new_board();
    lobby.status = LobbyStatus::AwaitingPlayers;
    lobby.players = HashMap::new();
    lobby.tick = 0;
}

pub fn pick_available_starting_coordinates(board: &Vec<Vec<Tile>>) -> (usize, usize) {
    let mut rng = rand::thread_rng();
    loop {
        let x = rng.gen_range(0..board.len());
        let y = rng.gen_range(0..board[0].len());
        match board[x][y].status {
            TileStatus::Empty => return (x, y),
            _ => continue,
        }
    }
}

enum OutcomeAssault {
    AttackingSameTile, // happens typically on side of the board, when going into a wall
    BlockedByMountain,
    NotEnoughTroops,
    TileNotOwned,
    SelfTroopsMove,
    ConquerEmpty,
    Tie,
    Victory(String, usize), // loser_uuid, how many invaders survive on defensive case
    Defeat(usize),          // how many loss on defensive case (1 remaining on attacking)
    VictoryCastle(usize),
}

fn resolve_assault(
    real_attacker_uuid: String,
    board: &[Vec<Tile>],
    attacker_xy: (usize, usize),
    defender_xy: (usize, usize),
) -> OutcomeAssault {
    if attacker_xy.0 == defender_xy.0 && attacker_xy.1 == defender_xy.1 {
        return OutcomeAssault::AttackingSameTile;
    }
    let attacking_board = &board[attacker_xy.0][attacker_xy.1];
    let defending_board = &board[defender_xy.0][defender_xy.1];
    let nb_attacking_troops = attacking_board.nb_troops.saturating_sub(1); // 1 troop must remain on the current case
    let nb_defending_troops = defending_board.nb_troops;
    if nb_attacking_troops == 0 {
        return OutcomeAssault::NotEnoughTroops;
    }
    if defending_board.tile_type == TileType::Mountain {
        return OutcomeAssault::BlockedByMountain;
    }
    // If a position move in the queue was stolen, the player_uuid of the attacking
    // tile will be different from the real attacker, and the attack forbidden
    match attacking_board.player_uuid.clone() {
        None => return OutcomeAssault::TileNotOwned,
        Some(attacker_uuid) => {
            if attacker_uuid != real_attacker_uuid {
                return OutcomeAssault::TileNotOwned;
            }
        }
    }
    if attacking_board.player_uuid == defending_board.player_uuid {
        return OutcomeAssault::SelfTroopsMove;
    }

    match defending_board.status {
        TileStatus::Occupied => match nb_attacking_troops.cmp(&nb_defending_troops) {
            Ordering::Equal => OutcomeAssault::Tie,
            Ordering::Greater => OutcomeAssault::Victory(
                defending_board
                    .player_uuid
                    .clone()
                    .expect("no defender name on attacked tile"),
                nb_attacking_troops - nb_defending_troops,
            ),
            Ordering::Less => OutcomeAssault::Defeat(nb_defending_troops - nb_attacking_troops),
        },
        TileStatus::Empty if defending_board.tile_type == TileType::Castle => {
            match nb_attacking_troops.cmp(&nb_defending_troops) {
                Ordering::Equal => OutcomeAssault::Tie,
                Ordering::Greater => {
                    OutcomeAssault::VictoryCastle(nb_attacking_troops - nb_defending_troops)
                }
                Ordering::Less => OutcomeAssault::Defeat(nb_defending_troops - nb_attacking_troops),
            }
        }
        TileStatus::Empty => OutcomeAssault::ConquerEmpty,
    }
}
