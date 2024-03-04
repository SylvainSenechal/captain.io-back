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
use std::{clone, collections::HashMap, sync::Arc};
use tokio::time::{interval, Duration};

use super::player_service::{Color, PlayerMove};

// todo : becareful if user join lobby and then quit before start : remove their game position
pub async fn game_loop(state: Arc<configs::app_state::AppState>) {
    let mut tick = 0;
    let mut interval = interval(Duration::from_millis(500));
    loop {
        interval.tick().await; // The first tick completes immediately
        tick = tick + 1;
        for mutex_lobby in state.lobbies.iter() {
            let mut lobby = mutex_lobby.lock().expect("failed to lock lobby");
            match lobby.status {
                LobbyStatus::AwaitingPlayers => (),
                LobbyStatus::StartingSoon => match lunch_game(state.clone(), &mut lobby) {
                    Ok(need_global_lobbies_update) => {
                        if need_global_lobbies_update {
                            drop(lobby); // global lobbies update needs to take ownership of all the lobbies
                            websocket_service::global_lobbies_update(state.clone());
                        }
                    }
                    Err(err) => println!("err starting lobby: {}", err), // todo : starting time is non existent, free lobby and player
                },
                LobbyStatus::InGame => {
                    tick_game(&mut lobby, tick, state.clone());
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
            let mut players = state.players.lock().expect("failed to lock player");
            for player_name in lobby.players.iter() {
                let new_player_color = Color::pick_available_color(&unavailable_colors);
                let player = players
                    .get_mut(&player_name.clone())
                    .expect("failed to get player");
                player.color = new_player_color.clone();
                unavailable_colors.push(new_player_color);
                player.current_position = pick_available_starting_coordinates(&lobby.board_game);
                lobby.board_game[player.current_position.0][player.current_position.1] = Tile {
                    status: TileStatus::Occupied,
                    tile_type: TileType::Kingdom,
                    nb_troops: 1,
                    player_name: Some(player.name.clone()),
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
) -> Result<(), String> {
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
    let mut players = state.players.lock().expect("failed to lock players");
    let mut scoreboard: HashMap<String, PlayerScore> = HashMap::new();
    for player_name in lobby.players.iter() {
        let player = players.get_mut(player_name).expect("player doesnt exists");
        scoreboard.insert(
            player_name.clone(),
            PlayerScore {
                total_positions: 0,
                total_troops: 0,
                color: player.color.clone(),
            },
        );
        if let Some(next_move) = player.queued_moves.pop_front() {
            let mut attacked_x = player.current_position.0;
            let mut attacked_y = player.current_position.1;
            match next_move {
                // todo : overflow : border of map : use saturating_sub/add ?
                PlayerMove::Left => {
                    attacked_x = player.current_position.0 - 1;
                }
                PlayerMove::Right => {
                    attacked_x = player.current_position.0 + 1;
                }
                PlayerMove::Up => {
                    attacked_y = player.current_position.1 - 1;
                }
                PlayerMove::Down => {
                    attacked_y = player.current_position.1 + 1;
                }
            }
            match resolve_assault(
                player.name.clone(),
                &lobby.board_game[player.current_position.0][player.current_position.1],
                &lobby.board_game[attacked_x][attacked_y],
            ) {
                IssueAssault::NotEnoughTroops => (),
                IssueAssault::TileNotOwned => (),
                IssueAssault::TroopsMove => {
                    lobby.board_game[attacked_x][attacked_y].nb_troops += lobby.board_game
                        [player.current_position.0][player.current_position.1]
                        .nb_troops
                        - 1;
                    lobby.board_game[player.current_position.0][player.current_position.1]
                        .nb_troops = 1;
                    player.current_position = (attacked_x, attacked_y);
                }
                IssueAssault::ConquerEmpty => {
                    lobby.board_game[attacked_x][attacked_y] = Tile {
                        status: TileStatus::Occupied,
                        tile_type: lobby.board_game[attacked_x][attacked_y].tile_type.clone(),
                        player_name: Some(player.name.clone()),
                        nb_troops: lobby.board_game[player.current_position.0]
                            [player.current_position.1]
                            .nb_troops
                            - 1,
                    };
                    lobby.board_game[player.current_position.0][player.current_position.1]
                        .nb_troops = 1;
                    player.current_position = (attacked_x, attacked_y);
                }
                IssueAssault::Tie => {
                    lobby.board_game[player.current_position.0][player.current_position.1]
                        .nb_troops = 1;
                    lobby.board_game[attacked_x][attacked_y].nb_troops = 0;
                }
                IssueAssault::Victory(loser_name, nb_remaining) => {
                    lobby.board_game[player.current_position.0][player.current_position.1]
                        .nb_troops = 1;
                    lobby.board_game[attacked_x][attacked_y] = Tile {
                        status: TileStatus::Occupied,
                        tile_type: lobby.board_game[attacked_x][attacked_y].tile_type.clone(),
                        player_name: Some(player.name.clone()),
                        nb_troops: nb_remaining,
                    };
                    if lobby.board_game[attacked_x][attacked_y].tile_type == TileType::Kingdom {
                        lobby.board_game[attacked_x][attacked_y].tile_type = TileType::Castle;
                        // todo : stop dead player from moving
                        for position in lobby.board_game.iter_mut().flatten() {
                            if let Some(occupier_name) = position.player_name.clone() {
                                if occupier_name == loser_name {
                                    position.player_name = Some(player.name.clone());
                                }
                            }
                        }
                    }
                    player.current_position = (attacked_x, attacked_y);
                }
                IssueAssault::Defeat(defensive_losses) => {
                    lobby.board_game[player.current_position.0][player.current_position.1]
                        .nb_troops = 1;
                    lobby.board_game[attacked_x][attacked_y].nb_troops = defensive_losses;
                }
            }
            println!("next move: {:?}", next_move);
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
    let _ = lobby
        .lobby_broadcast
        .send(WsMessageToClient::GameUpdate(GameUpdate {
            board_game: lobby.board_game.clone(),
            score_board: scoreboard,
        }));

    Ok(())
}

pub fn pick_available_starting_coordinates(board: &Vec<Vec<Tile>>) -> (usize, usize) {
    let mut rng = rand::thread_rng();
    loop {
        let x = rng.gen_range(0..board.len()); // todo : be careful if x/y inverted
        let y = rng.gen_range(0..board[0].len());
        match board[x][y].status {
            TileStatus::Empty => return (x, y),
            _ => continue,
        }
    }
}

enum IssueAssault {
    NotEnoughTroops,
    TileNotOwned,
    TroopsMove,
    ConquerEmpty,
    Tie,
    Victory(String, usize), // loser_name, how many invaders survive on defensive case
    Defeat(usize),          // how many loss on defensive case (1 remaining on attacking)
}

fn resolve_assault(
    real_attacker_name: String,
    attacking_board: &Tile,
    defending_board: &Tile,
) -> IssueAssault {
    let nb_attacking_troops = attacking_board.nb_troops.saturating_sub(1); // 1 troop must remain on the current case
    let nb_defending_troops = defending_board.nb_troops;
    if nb_attacking_troops == 0 {
        return IssueAssault::NotEnoughTroops;
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
        return IssueAssault::TroopsMove;
    }

    match defending_board.status {
        TileStatus::Empty => IssueAssault::ConquerEmpty,
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
    }
}
