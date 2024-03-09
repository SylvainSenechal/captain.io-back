use crate::{
    configs::{
        self,
        app_state::{Lobby, LobbyStatus, Tile, TileStatus, TileType},
    },
    models::messages_to_clients::{GameUpdate, PlayerScore, WsMessageToClient},
    service_layer::websocket_service,
};
use chrono::Utc;
use rand::Rng;
use std::{
    clone,
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};
use tokio::time::{interval, Duration};

use super::{
    player_service::{Color, PlayerMove, PlayerMoves},
    websocket_service::global_lobbies_update,
};

// todo : handle when tip of queue is stolen

// todo : becareful if user join lobby and then quit before start : remove their game position
// + test reload during the game
pub async fn game_loop(state: Arc<configs::app_state::AppState>) {
    let mut tick = 0;
    let mut interval = interval(Duration::from_millis(500));
    loop {
        interval.tick().await; // The first tick completes immediately
        tick = tick + 1;
        for mutex_lobby in state.lobbies.iter() {
            let mut lobby = mutex_lobby.write().expect("failed to lock lobby");
            match lobby.status {
                LobbyStatus::AwaitingPlayers => (),
                LobbyStatus::StartingSoon => match lunch_game(state.clone(), &mut lobby) {
                    Ok(need_global_lobbies_update) => {
                        if need_global_lobbies_update {
                            drop(lobby); // global lobbies update needs to take ownership of all the lobbies
                            global_lobbies_update(state.clone());
                        }
                    }
                    Err(err) => println!("err starting lobby: {}", err), // todo : starting time is non existent, free lobby and player
                },
                LobbyStatus::InGame => {
                    if let Ok(is_game_finished) = tick_game(&mut lobby, tick, state.clone()) {
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

fn lunch_game(state: Arc<configs::app_state::AppState>, lobby: &mut Lobby) -> Result<bool, String> {
    if let Some(starting_time) = lobby.next_starting_time {
        if starting_time - Utc::now().timestamp() <= 0 {
            lobby.status = LobbyStatus::InGame;
            let mut unavailable_colors = vec![];
            let mut players = state.players.write().expect("failed to lock player");
            for (player_name, _uuid) in lobby.players.iter() {
                let new_player_color = Color::pick_available_color(&unavailable_colors);
                let player = players
                    .get_mut(&player_name.clone())
                    .expect("failed to get player");
                player.queued_moves = VecDeque::new();
                player.color = new_player_color.clone();
                unavailable_colors.push(new_player_color);
                player.xy = pick_available_starting_coordinates(&lobby.board_game);
                lobby.board_game[player.xy.0][player.xy.1] = Tile {
                    status: TileStatus::Occupied,
                    tile_type: TileType::Kingdom,
                    nb_troops: 1,
                    player_name: Some(player.name.clone()),
                    hidden: true,
                };
            }
            let _ = lobby
                .lobby_broadcast
                .send(WsMessageToClient::GameStarted(lobby.lobby_id));

            return Ok(true);
        } else {
            Ok(false)
        }
    } else {
        Err(format!(
            "lobby is not in Starting soon state, state:{:?}",
            lobby.status
        ))
    }
}

// todo  full hexagon circle : ++ generation bonus
// todo max move queue player : 20
fn tick_game(
    lobby: &mut Lobby,
    tick: usize,
    state: Arc<configs::app_state::AppState>,
) -> Result<bool, String> {
    for position in lobby.board_game.iter_mut().flatten() {
        if tick % 5 == 0 {
            // todo : use const
            match position.status {
                TileStatus::Occupied => match position.tile_type {
                    TileType::Blank => position.nb_troops += 1,
                    TileType::Kingdom => position.nb_troops += 5,
                    TileType::Castle => position.nb_troops += 2,
                    TileType::Mountain => (),
                },
                _ => (),
            }
        }
    }
    let mut players = state.players.write().expect("failed to lock players");
    let mut scoreboard: HashMap<String, PlayerScore> = HashMap::new();
    let mut remaining_players = HashSet::new();
    let width_game = lobby.board_game.len();
    let height_game = lobby.board_game[0].len();
    for (player_name, _uuid) in lobby.players.iter() {
        scoreboard.insert(
            player_name.clone(),
            PlayerScore {
                total_positions: 0,
                total_troops: 0,
                color: Color::Grey,
            },
        );
        if let Some(attacker) = players.get_mut(player_name) {
            // attacker = None if the player left the game
            scoreboard
                .get_mut(&attacker.name.clone())
                .expect("no attacker name in score board")
                .color = attacker.color.clone();

            // todo : if move is useless, try to perform the next one ?
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
                    attacker.name.clone(),
                    &lobby.board_game,
                    attacker.xy,
                    (attacked_x, attacked_y),
                ) {
                    IssueAssault::AttackingSameTile => (),
                    IssueAssault::BlockedByMountain => (),
                    IssueAssault::NotEnoughTroops => (),
                    IssueAssault::TileNotOwned => (),
                    IssueAssault::SelfTroopsMove => {
                        lobby.board_game[attacked_x][attacked_y].nb_troops +=
                            lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops - 1;
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                        attacker.xy = (attacked_x, attacked_y);
                    }
                    IssueAssault::ConquerEmpty => {
                        lobby.board_game[attacked_x][attacked_y] = Tile {
                            status: TileStatus::Occupied,
                            tile_type: lobby.board_game[attacked_x][attacked_y].tile_type.clone(),
                            player_name: Some(attacker.name.clone()),
                            nb_troops: lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops - 1,
                            hidden: true,
                        };
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                        attacker.xy = (attacked_x, attacked_y);
                    }
                    IssueAssault::Tie => {
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                        lobby.board_game[attacked_x][attacked_y].nb_troops = 0;
                    }
                    IssueAssault::Victory(loser_name, nb_remaining) => {
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                        lobby.board_game[attacked_x][attacked_y] = Tile {
                            status: TileStatus::Occupied,
                            tile_type: lobby.board_game[attacked_x][attacked_y].tile_type.clone(),
                            player_name: Some(attacker.name.clone()),
                            nb_troops: nb_remaining,
                            hidden: true,
                        };
                        if lobby.board_game[attacked_x][attacked_y].tile_type == TileType::Kingdom {
                            lobby.board_game[attacked_x][attacked_y].tile_type = TileType::Castle;
                            for position in lobby.board_game.iter_mut().flatten() {
                                if let Some(occupier_name) = position.player_name.clone() {
                                    if occupier_name == loser_name {
                                        position.player_name = Some(attacker.name.clone());
                                    }
                                    remaining_players.insert(
                                        position
                                            .player_name
                                            .clone()
                                            .expect("no player in position"),
                                    );
                                }
                            }
                        }
                        attacker.xy = (attacked_x, attacked_y);
                    }
                    IssueAssault::Defeat(defensive_losses) => {
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                        lobby.board_game[attacked_x][attacked_y].nb_troops = defensive_losses;
                    }
                    IssueAssault::VictoryCastle(nb_remaining) => {
                        lobby.board_game[attacker.xy.0][attacker.xy.1].nb_troops = 1;
                        lobby.board_game[attacked_x][attacked_y] = Tile {
                            status: TileStatus::Occupied,
                            tile_type: lobby.board_game[attacked_x][attacked_y].tile_type.clone(),
                            player_name: Some(attacker.name.clone()),
                            nb_troops: nb_remaining,
                            hidden: true,
                        };
                        attacker.xy = (attacked_x, attacked_y);
                    }
                }
                println!("next move: {:?}", next_move);
            }
        }
    }

    for position in lobby.board_game.iter().flatten() {
        if let Some(occupier_name) = position.player_name.clone() {
            scoreboard.entry(occupier_name).and_modify(|score| {
                score.total_positions += 1;
                score.total_troops = score.total_troops + position.nb_troops;
            });
        }
    }
    for (_, player) in players.iter() {
        let mut personal_board_game: Vec<Vec<Tile>> = vec![];
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
                column.push(Tile {
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
                if let Some(name) = lobby.board_game[i][j].player_name.clone() {
                    if name == player.name {
                        let min_w = i.saturating_sub(1);
                        let min_h = j.saturating_sub(1);
                        let max_w = (i + 1).min(width - 1);
                        let max_h = (j + 1).min(height - 1);

                        personal_board_game[min_w][min_h] = lobby.board_game[min_w][min_h].clone();
                        personal_board_game[min_w][j] = lobby.board_game[min_w][j].clone();
                        personal_board_game[min_w][max_h] = lobby.board_game[min_w][max_h].clone();
                        personal_board_game[i][min_h] = lobby.board_game[i][min_h].clone();
                        personal_board_game[i][j] = lobby.board_game[i][j].clone();
                        personal_board_game[i][max_h] = lobby.board_game[i][max_h].clone();
                        personal_board_game[max_w][min_h] = lobby.board_game[max_w][min_h].clone();
                        personal_board_game[max_w][j] = lobby.board_game[max_w][j].clone();
                        personal_board_game[max_w][max_h] = lobby.board_game[max_w][max_h].clone();

                        personal_board_game[min_w][min_h].hidden = false;
                        personal_board_game[min_w][j].hidden = false;
                        personal_board_game[min_w][max_h].hidden = false;
                        personal_board_game[i][min_h].hidden = false;
                        personal_board_game[i][j].hidden = false;
                        personal_board_game[i][max_h].hidden = false;
                        personal_board_game[max_w][min_h].hidden = false;
                        personal_board_game[max_w][j].hidden = false;
                        personal_board_game[max_w][max_h].hidden = false;
                    }
                }
            }
        }

        let _ = player
            .personal_tx
            .send(WsMessageToClient::GameUpdate(GameUpdate {
                board_game: personal_board_game,
                // board_game: lobby.board_game.clone(),
                score_board: scoreboard.clone(),
                moves: PlayerMoves {
                    queued_moves: player.queued_moves.clone(),
                    xy: player.xy,
                },
            }));
    }

    if remaining_players.len() == 1 {
        let _ = lobby
            .lobby_broadcast
            .send(WsMessageToClient::WinnerAnnouncement(
                remaining_players
                    .iter()
                    .next()
                    .expect("no remaining player to win")
                    .to_string(),
            ));
        return Ok(true);
    }

    Ok(false)
}

pub fn end_lobby_game(lobby: &mut Lobby, state: Arc<configs::app_state::AppState>) {
    let mut all_players = state
        .players
        .write()
        .expect("failed to lock players end game");

    for (_, player_uuid) in lobby.players.iter() {
        for (_, player) in all_players.iter_mut() {
            if player_uuid.clone() == player.uuid {
                if let Some(already_in_lobby_id) = player.playing_in_lobby {
                    if already_in_lobby_id == lobby.lobby_id {
                        player.playing_in_lobby = None;
                    }
                }
            }
        }
    }
    lobby.generate_new_board();
    lobby.status = LobbyStatus::AwaitingPlayers;
    lobby.players = HashMap::new();
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

enum IssueAssault {
    AttackingSameTile, // happens typically on side of the board, when going into a wall
    BlockedByMountain,
    NotEnoughTroops,
    TileNotOwned,
    SelfTroopsMove,
    ConquerEmpty,
    Tie,
    Victory(String, usize), // loser_name, how many invaders survive on defensive case
    Defeat(usize),          // how many loss on defensive case (1 remaining on attacking)
    VictoryCastle(usize),
}

fn resolve_assault(
    real_attacker_name: String,
    board: &Vec<Vec<Tile>>,
    attacker_xy: (usize, usize),
    defender_xy: (usize, usize),
) -> IssueAssault {
    if attacker_xy.0 == defender_xy.0 && attacker_xy.1 == defender_xy.1 {
        return IssueAssault::AttackingSameTile;
    }
    let attacking_board = &board[attacker_xy.0][attacker_xy.1];
    let defending_board = &board[defender_xy.0][defender_xy.1];
    let nb_attacking_troops = attacking_board.nb_troops.saturating_sub(1); // 1 troop must remain on the current case
    let nb_defending_troops = defending_board.nb_troops;
    if nb_attacking_troops == 0 {
        return IssueAssault::NotEnoughTroops;
    }
    if defending_board.tile_type == TileType::Mountain {
        return IssueAssault::BlockedByMountain;
    }
    // If a position move in the queue was stolen,  the player_name of the attacking
    // tile will be different from the real attacker, and the attack forbidden
    match attacking_board.player_name.clone() {
        None => return IssueAssault::TileNotOwned,
        Some(attacker_name) => {
            if attacker_name != real_attacker_name {
                return IssueAssault::TileNotOwned;
            }
        }
    }
    if attacking_board.player_name == defending_board.player_name {
        return IssueAssault::SelfTroopsMove;
    }

    match defending_board.status {
        TileStatus::Occupied => {
            if nb_attacking_troops == nb_defending_troops {
                return IssueAssault::Tie;
            } else if nb_attacking_troops > nb_defending_troops {
                return IssueAssault::Victory(
                    defending_board
                        .player_name
                        .clone()
                        .expect("no defender name on attacked tile"),
                    nb_attacking_troops - nb_defending_troops,
                );
            } else {
                return IssueAssault::Defeat(nb_defending_troops - nb_attacking_troops);
            }
        }
        // todo : redundant code between 2 arms
        TileStatus::Empty if defending_board.tile_type == TileType::Castle => {
            if nb_attacking_troops == nb_defending_troops {
                return IssueAssault::Tie;
            } else if nb_attacking_troops > nb_defending_troops {
                return IssueAssault::VictoryCastle(nb_attacking_troops - nb_defending_troops);
            } else {
                return IssueAssault::Defeat(nb_defending_troops - nb_attacking_troops);
            }
        }
        TileStatus::Empty => IssueAssault::ConquerEmpty,
    }
}
