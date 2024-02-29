use std::{borrow::Borrow, collections::HashMap, sync::Arc};

use crate::{
    configs::{
        self,
        app_state::{Lobby, LobbyStatus, Tile, TileStatus},
    },
    models::messages_to_clients::{GameUpdate, PlayerScore, WsMessageToClient},
    service_layer::websocket_service,
};
use chrono::Utc;
use rand::{seq::SliceRandom, Rng};
use tokio::time::{sleep, Duration, Instant};

use super::player_service::PlayerMove;

pub async fn game_loop(state: Arc<configs::app_state::AppState>) {
    // todo : use interval instead of sleep https://docs.rs/tokio/latest/tokio/time/fn.interval.html
    // be careful if the game loop is longer than the interval
    let mut tick = 0;
    loop {
        println! {"loop {}", tick};
        tick = tick + 1;
        sleep(Duration::from_millis(500)).await;
        for mutex_lobby in state.lobbies.iter() {
            let mut lobby = mutex_lobby.lock().expect("failed to lock lobby");
            match lobby.status {
                LobbyStatus::AwaitingPlayers => (),
                LobbyStatus::StartingSoon => match lunch_game(&mut lobby) {
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

fn lunch_game(lobby: &mut Lobby) -> Result<bool, String> {
    if let Some(starting_time) = lobby.next_starting_time {
        if starting_time - Utc::now().timestamp() <= 0 {
            lobby
                .lobby_broadcast
                .send(WsMessageToClient::GameStarted(lobby.lobby_id))
                .expect("failed to notify game started");

            lobby.status = LobbyStatus::InGame;
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
            println!("tick {}", tick);
            match position.status {
                TileStatus::Occupied => {
                    // todo : other things to check : don't increase if it's neutral..
                    position.nb_troops += 1;
                }
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
        println!("len next move: {:?}", player.queued_moves.len());
        // let a = lobby.board_game[0][0];
        if let Some(next_move) = player.queued_moves.pop_front() {
            // todo : deal with attacking same territory..
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
                &lobby.board_game[player.current_position.0][player.current_position.1],
                &lobby.board_game[attacked_x][attacked_y],
            ) {
                IssueAssault::NotEnoughTroops => (),
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
                IssueAssault::Victory(nb_remaining) => {
                    lobby.board_game[player.current_position.0][player.current_position.1]
                        .nb_troops = 1;
                    lobby.board_game[attacked_x][attacked_y] = Tile {
                        status: TileStatus::Occupied,
                        tile_type: lobby.board_game[attacked_x][attacked_y].tile_type.clone(),
                        player_name: Some(player.name.clone()),
                        nb_troops: nb_remaining,
                    };
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
            // if position.position_type == PositionType::Occupied {
            scoreboard.entry(occupier_name).and_modify(|score| {
                score.total_positions += 1;
                score.total_troops = score.total_troops + position.nb_troops;
            });
            // .or_insert(PlayerScore {
            //     total_positions: 1,
            //     total_troops: *nb_troops,
            // });
        }
    }
    lobby
        .lobby_broadcast
        .send(WsMessageToClient::GameUpdate(GameUpdate {
            board_game: lobby.board_game.clone(),
            score_board: scoreboard,
        }))
        .expect("failed to update game");

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
    TroopsMove,
    ConquerEmpty,
    Tie,
    Victory(usize), // how many invaders survive on defensive case
    Defeat(usize),  // how many loss on defensive case (1 remaining on attacking)
}

fn resolve_assault(attacking_board: &Tile, defending_board: &Tile) -> IssueAssault {
    let nb_attacking_troops = attacking_board.nb_troops.saturating_sub(1); // 1 troop must remain on the current case
    let nb_defending_troops = defending_board.nb_troops;
    if nb_attacking_troops == 0 {
        return IssueAssault::NotEnoughTroops;
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
                return IssueAssault::Victory(nb_attacking_troops - nb_defending_troops);
            } else {
                return IssueAssault::Defeat(nb_defending_troops - nb_attacking_troops);
            }
        }
    }
}
