use crate::configs;
use crate::configs::app_state::{ChatMessage, LobbyStatus, Tile, TileStatus, TileType};
use crate::constants::constants::{DELAY_FOR_GAMESTART_SEC, NB_LOBBIES};
use crate::data_access_layer::player_dal::{self, Player};
use crate::models::messages_from_clients::ClientCommand;
use crate::models::{
    messages_to_clients::LobbiesGeneralUpdate, messages_to_clients::LobbyGeneralUpdate,
    messages_to_clients::WsMessageToClient,
};
use crate::service_layer::game_service::pick_available_starting_coordinates;
use crate::service_layer::player_service::{self, Color};
use axum::extract::ws::{Message, WebSocket};
use chrono::Utc;
use futures_util::{
    sink::SinkExt,
    stream::{SplitStream, StreamExt},
};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::broadcast::{self, Receiver};

pub async fn handle_websocket(
    player_uuid: String,
    socket: WebSocket,
    state: Arc<configs::app_state::AppState>,
) {
    let (mut sender, mut receiver) = socket.split();

    // todo : return / timeout after x seconds
    let perso_tx = broadcast::channel(100).0;

    let player_in_db = player_dal::get_player_by_uuid(&state, player_uuid.clone());
    if let Err(err) = player_in_db {
        println!(
            "handle_websocket, can't find player with uuid={}, err={:?} ",
            player_uuid, err
        );
        return;
    }
    let player = player_in_db.unwrap();
    // todo : handle if player is already inside (handle double windows opened)
    state
        .players
        .lock()
        .expect("couldnt lock players mutex")
        .insert(
            player.name.clone(),
            player_service::Player {
                uuid: player.uuid.clone(),
                name: player.name.clone(),
                personal_tx: perso_tx.clone(),
                playing_in_lobby: None,
                queued_moves: VecDeque::new(),
                current_position: (2, 2),
                color: Color::Red,
            },
        );

    println!("CURRENT PLAYERS {:?}", state.players);

    let mut global_subscription = state.global_broadcast.subscribe();
    let mut personal_subscription = perso_tx.subscribe();
    let mut lobby_subscription: Receiver<WsMessageToClient> = broadcast::channel(100).1;
    let cloned_state = state.clone();
    let cloned_player = Player {
        uuid: player.uuid.clone(),
        name: player.name.clone(),
    };
    let mut handle1 =
        tokio::spawn(async move { receive(&mut receiver, player.name, cloned_state).await });

    global_lobbies_update(state.clone());
    global_chat_sync(state.clone());

    let cloned_state = state.clone();
    let mut handle2 = tokio::spawn(async move {
        loop {
            tokio::select! {
                elem = global_subscription.recv() => {
                    match elem {
                        Ok(msg) => {
                            println!("global msg {:?}", msg);
                            match msg {
                                _ => sender.send(msg.to_string_message()).await.expect("failed to send to global sub")
                            };
                        },
                        Err(e) => {
                           println!("eeee 1 {}", e);
                        },
                    }
                }
                elem = personal_subscription.recv() => {
                    match elem {
                        Ok(msg) => {
                            println!("sending personal message {:?}", msg);
                            match msg {
                                WsMessageToClient::JoinLobby(lobby_id) => {
                                    // todo : check bound lobby id
                                    println!("sending personal join lobby, {:?}", msg.to_string_message());
                                    lobby_subscription = cloned_state.lobbies[lobby_id].lock().unwrap().lobby_broadcast.subscribe();
                                    sender.send(msg.to_string_message()).await.expect("failed to send to global sub");
                                },
                                _ => sender.send(msg.to_string_message()).await.expect("failed to send to personal sub")
                            };
                        },
                        Err(e) => {
                        //    println!("eeee 2 {}", e);
                        }
                    }
                },
                elem = lobby_subscription.recv() => {
                    match elem {
                        Ok(msg) => {
                            println!("sending lobby message {:?}", msg);
                            match msg {
                                _ => sender.send(msg.to_string_message()).await.expect("failed to send to lobby sub")
                            };
                        },
                        Err(e) => {
                        //    println!("eeee 3 {}", e);
                        }
                    }
                },
                else => {
                    println!("BREAK");
                    break
                },
            }
        }
    });

    // Waiting while the player is connected
    tokio::select! {
        res = (&mut handle1) => {
            println!("aborted 1 {:?}", res);
            handle2.abort()
        },
        res = (&mut handle2) => {
            println!("aborted 2 {:?}", res);
            handle1.abort()
        },
    };

    // Logic after disconnected player
    // todo : useless if I want to keep score when player leave but game continues ?
    if let Some(lobby_id) = state
        .players
        .lock()
        .expect("failed to lock players")
        .get(&cloned_player.name)
        .expect("failed to get player by name")
        .playing_in_lobby
    {
        state.lobbies[lobby_id]
            .lock()
            .unwrap()
            .players
            .remove(&cloned_player.name);
    }

    state
        .players
        .lock()
        .expect("failed to lock players to remove disconnected")
        .remove(&cloned_player.name)
        .expect("failed to remove player from players list");
    global_lobbies_update(state.clone());
}

async fn receive(
    receiver: &mut SplitStream<WebSocket>,
    player_name: String,
    state: Arc<configs::app_state::AppState>,
) {
    // todo : add disconnected for afk, spawn task waiter, reset to 0 after each message, if time > 100 : return err afk
    'rec_v_loop: while let Some(Ok(message)) = receiver.next().await {
        match message {
            Message::Text(msg) => {
                let command = msg.parse::<ClientCommand>();
                println!("new command {:?}", command);
                if let Ok(c) = command {
                    match c {
                        ClientCommand::Move(new_move) => state
                            .players
                            .lock()
                            .expect("failed to lock players")
                            .get_mut(&player_name)
                            .expect("player not found")
                            .queued_moves
                            .push_back(new_move),
                        // todo : limit queue size
                        ClientCommand::JoinLobby(join_lobby_id) => {
                            println!("JOIN LOBBY {:?}", join_lobby_id);
                            if join_lobby_id < NB_LOBBIES {
                                let mut players =
                                    state.players.lock().expect("failed to lock players");
                                let mut lobby_to_join = state.lobbies[join_lobby_id]
                                    .lock()
                                    .expect("failed ot lock lobby");
                                if lobby_to_join.players.len() >= lobby_to_join.player_capacity {
                                    continue 'rec_v_loop;
                                }
                                if lobby_to_join.status != LobbyStatus::AwaitingPlayers {
                                    continue 'rec_v_loop;
                                }
                                let mut unavailable_colors = vec![];
                                for player in lobby_to_join.players.iter() {
                                    unavailable_colors.push(
                                        players
                                            .get(player)
                                            .expect("failed to get player for color")
                                            .color
                                            .clone(),
                                    );
                                }
                                let new_player_color =
                                    Color::pick_available_color(unavailable_colors);
                                let player = players
                                    .get_mut(&player_name)
                                    .expect("failed to get playername");
                                if let Some(in_lobby) = player.playing_in_lobby {
                                    if in_lobby == join_lobby_id {
                                        // don't join a lobby you're already in
                                        continue 'rec_v_loop;
                                    }
                                    // remove from current lobby before joining the new one
                                    state.lobbies[in_lobby]
                                        .lock()
                                        .unwrap()
                                        .players
                                        .remove(&player_name.clone());
                                }
                                lobby_to_join.players.insert(player_name.clone());
                                player.playing_in_lobby = Some(join_lobby_id);
                                player.color = new_player_color;
                                player.current_position =
                                    pick_available_starting_coordinates(&lobby_to_join.board_game);
                                lobby_to_join.board_game[player.current_position.0]
                                    [player.current_position.1] = Tile {
                                    status: TileStatus::Occupied,
                                    tile_type: TileType::Kingdom,
                                    nb_troops: 22,
                                    player_name: Some(player.name.clone()),
                                };
                                if lobby_to_join.players.len() == lobby_to_join.player_capacity {
                                    // Start the game soon..
                                    println!("lobby {} is starting", lobby_to_join.lobby_id);
                                    lobby_to_join.status = LobbyStatus::StartingSoon;
                                    lobby_to_join.next_starting_time =
                                        Some(Utc::now().timestamp() + DELAY_FOR_GAMESTART_SEC);
                                }
                                drop(lobby_to_join);
                                player
                                    .personal_tx
                                    .send(WsMessageToClient::JoinLobby(join_lobby_id))
                                    .expect("failed to notify joined lobby");
                                player
                                    .personal_tx
                                    .send(WsMessageToClient::LobbyChatSync(
                                        state.lobbies[join_lobby_id]
                                            .lock()
                                            .expect("failed to lock lobby")
                                            .messages
                                            .to_owned(),
                                    ))
                                    .expect("failed to notify current lobby chat");
                                drop(players); // unlock players because we are trying to lock it in global_lobby_update
                                global_lobbies_update(state.clone());
                            }
                        }
                        ClientCommand::Ping => {
                            state
                                .players
                                .lock()
                                .expect("couldnt lock players")
                                .get(&player_name)
                                .expect("failed to get playername")
                                .personal_tx
                                .send(WsMessageToClient::Pong)
                                .expect("failed to pong player");
                        }
                        ClientCommand::SendGlobalMessage(message) => {
                            state
                                .global_chat_messages
                                .lock()
                                .expect("failed to lock global chat")
                                .push(ChatMessage {
                                    message: message.clone(),
                                });
                            global_chat_new_message(state.clone(), message);
                        }
                        ClientCommand::SendLobbyMessage(message) => {
                            if let Some(lobby_player) = state
                                .players
                                .lock()
                                .expect("couldnt lock players")
                                .get(&player_name)
                                .expect("couldnt find playername")
                                .playing_in_lobby
                            {
                                // can only send messages in the lobby youre in
                                state.lobbies[lobby_player]
                                    .lock()
                                    .expect("failed ot lock lobby")
                                    .messages
                                    .push(ChatMessage {
                                        message: message.clone(),
                                    });
                                state.lobbies[lobby_player]
                                    .lock()
                                    .unwrap()
                                    .lobby_broadcast
                                    .send(WsMessageToClient::LobbyChatNewMessage(ChatMessage {
                                        message: message,
                                    }))
                                    .expect("failed to notify of new lobby message");
                            }
                        }
                    }
                }
            }
            _ => {
                println!("received unexpected message {:?}", message);
                break 'rec_v_loop;
            }
        }
    }
}

pub fn global_lobbies_update(state: Arc<configs::app_state::AppState>) {
    let mut update: LobbiesGeneralUpdate = LobbiesGeneralUpdate {
        total_players_connected: state.players.lock().expect("failed to lock players").len(),
        lobbies: vec![],
    };

    for lobby in state.lobbies.iter() {
        let lobby = lobby.lock().expect("failed to lock lobby");
        update.lobbies.push(LobbyGeneralUpdate {
            player_capacity: lobby.player_capacity,
            nb_connected: lobby.players.len(),
            status: lobby.status,
            next_starting_time: lobby.next_starting_time,
        });
    }
    state
        .global_broadcast
        .send(WsMessageToClient::LobbiesUpdate(update))
        .expect("global lobbies update failed");
}

fn global_chat_sync(state: Arc<configs::app_state::AppState>) {
    state
        .global_broadcast // todo : sync chat should be personnal ??
        .send(WsMessageToClient::GlobalChatSync(
            state
                .global_chat_messages
                .lock()
                .expect("failed to lock global chat")
                .to_owned(),
        ))
        .expect("global chat sync failed");
}
fn global_chat_new_message(state: Arc<configs::app_state::AppState>, message: String) {
    state
        .global_broadcast
        .send(WsMessageToClient::GlobalChatNewMessage(ChatMessage {
            message: message,
        }))
        .expect("global chat new message failed");
}
