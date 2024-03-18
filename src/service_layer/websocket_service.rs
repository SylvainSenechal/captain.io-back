use crate::configs;
use crate::configs::app_state::{ChatMessage, LobbyStatus};
use crate::constants::{
    DELAY_FOR_GAMESTART_SEC, DISPLAY_N_LAST_MESSAGES, MAX_QUEUED_MOVES, NB_LOBBIES,
};
use crate::data_access_layer::player_dal::{self, Player};
use crate::models::messages_from_clients::ClientCommand;
use crate::models::{
    messages_to_clients::LobbiesGeneralUpdate, messages_to_clients::LobbyGeneralUpdate,
    messages_to_clients::WsMessageToClient,
};
use crate::service_layer::player_service::{self, Color};
use axum::extract::ws::{Message, WebSocket};
use chrono::Utc;
use futures_util::{
    sink::SinkExt,
    stream::{SplitStream, StreamExt},
};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::broadcast::{self, Receiver, Sender};

pub async fn handle_websocket(
    player: Player,
    socket: WebSocket,
    state: Arc<configs::app_state::AppState>,
) {
    let (mut sender, mut receiver) = socket.split();

    // todo : return / timeout after x seconds
    let (perso_tx, mut personal_subscription) = tokio::sync::mpsc::unbounded_channel();

    state
        .players
        .write()
        .expect("couldnt lock players mutex")
        .insert(
            player.uuid.clone(),
            player_service::Player {
                uuid: player.uuid.clone(),
                name: player.name.clone(),
                personal_tx: perso_tx.clone(),
                playing_in_lobby: None,
                queued_moves: VecDeque::new(),
                xy: (0, 0),
                color: Color::Red,
            },
        );

    println!("CURRENT PLAYERS {:?}", state.players);

    let mut global_subscription = state.global_broadcast.subscribe();
    let (_lobby_sender, mut lobby_subscription): (
        Sender<WsMessageToClient>,
        Receiver<WsMessageToClient>,
    ) = broadcast::channel(100);
    let cloned_player = Player {
        uuid: player.uuid.clone(),
        name: player.name.clone(),
    };
    let cloned_state = state.clone();
    let mut ws_receiver = tokio::spawn(async move {
        receive(
            &mut receiver,
            cloned_player.name,
            cloned_player.uuid,
            cloned_state,
        )
        .await
    });

    global_lobbies_update(state.clone());
    global_chat_sync(perso_tx, state.clone());

    let cloned_state = state.clone();
    let mut message_controler = tokio::spawn(async move {
        loop {
            tokio::select! {
                elem = global_subscription.recv() => {
                    // println!("global msg {:?}", elem);
                    match elem {
                        Ok(msg) => {
                            let _ = sender.send(msg.to_string_message()).await;
                        },
                        Err(e) => {
                           println!("eeee 1 {}", e);
                        },
                    }
                }
                elem = lobby_subscription.recv() => {
                    // println!("lobby msg {:?}", elem);
                    match elem {
                        Ok(msg) => {
                            let _ = sender.send(msg.to_string_message()).await;
                        },
                        Err(e) => {
                           println!("eeee 2 {}", e);
                        }
                    }
                },
                elem = personal_subscription.recv() => {
                    // println!("personal msg {:?}", elem);
                    match elem {
                        Some(msg) => {
                            match msg {
                                WsMessageToClient::JoinLobby(lobby_id) => {
                                    if lobby_id < NB_LOBBIES { // lobby_id 0 indexed
                                        lobby_subscription = cloned_state.lobbies[lobby_id].read().unwrap().lobby_broadcast.subscribe();
                                        let _ = sender.send(msg.to_string_message()).await;
                                    }
                                },
                                _ => {let _ = sender.send(msg.to_string_message()).await;}
                            };
                        },
                        None => {
                           println!("eeee 3 ");
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
        res = (&mut ws_receiver) => {
            println!("aborted ws_receiver {:?}", res);
            message_controler.abort()
        },
        res = (&mut message_controler) => {
            println!("aborted message_controler {:?}", res);
            ws_receiver.abort()
        },
    };

    // Handle player disconnecting :
    // 1. Remove from the lobby (except when already in game)
    if let Some(lobby_id) = state
        .players
        .read()
        .expect("failed to read players")
        .get(&player.uuid)
        .expect("player uuid not found")
        .playing_in_lobby
    {
        let lobby_status = state.lobbies[lobby_id]
            .read()
            .expect("failed to read lobby")
            .status;
        match lobby_status {
            LobbyStatus::InGame => (), // don't remove from lobby, the player will become inactive instead
            LobbyStatus::StartingSoon => (), // don't remove from lobby, the player will become inactive instead
            LobbyStatus::AwaitingPlayers => {
                state.lobbies[lobby_id]
                    .write()
                    .expect("failed to write lobby")
                    .players
                    .remove(&player.uuid);
            }
        }
    }

    // 2. Remove from connected players list
    state
        .players
        .write()
        .expect("failed to lock players to remove disconnected")
        .remove(&player.uuid.clone());
    global_lobbies_update(state.clone());
}

async fn receive(
    receiver: &mut SplitStream<WebSocket>,
    player_name: String, // todo : what if player changes name and is still connected ?
    player_uuid: String,
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
                        ClientCommand::Move(new_move) => {
                            let mut players =
                                state.players.write().expect("failed to lock players");
                            let player = players.get_mut(&player_uuid).expect("msg");
                            if player.queued_moves.len() < MAX_QUEUED_MOVES {
                                player.queued_moves.push_back(new_move)
                            }
                            player
                                .personal_tx
                                .send(WsMessageToClient::QueuedMoves(
                                    player_service::PlayerMoves {
                                        queued_moves: player.queued_moves.clone(),
                                        xy: player.xy,
                                    },
                                ))
                                .expect("failed to notify current lobby chat");
                        }
                        ClientCommand::JoinLobby(join_lobby_id) => {
                            println!("JOIN LOBBY {:?}", join_lobby_id);
                            if join_lobby_id < NB_LOBBIES {
                                let mut players =
                                    state.players.write().expect("failed to lock players");
                                let mut lobby_to_join = state.lobbies[join_lobby_id]
                                    .write()
                                    .expect("failed ot lock lobby");
                                if lobby_to_join.players.len() >= lobby_to_join.player_capacity {
                                    continue 'rec_v_loop;
                                }
                                if lobby_to_join.status != LobbyStatus::AwaitingPlayers {
                                    continue 'rec_v_loop;
                                }
                                let player = players
                                    .get_mut(&player_uuid)
                                    .expect("failed to get playername");
                                if let Some(in_lobby) = player.playing_in_lobby {
                                    if in_lobby == join_lobby_id {
                                        // don't join a lobby you're already in
                                        continue 'rec_v_loop;
                                    }
                                    // remove from current lobby before joining the new one
                                    state.lobbies[in_lobby]
                                        .write()
                                        .unwrap()
                                        .players
                                        .remove(&player_uuid.clone());
                                }
                                lobby_to_join
                                    .players
                                    .insert(player_uuid.clone(), player_name.clone());
                                player.playing_in_lobby = Some(join_lobby_id);
                                if lobby_to_join.players.len() == lobby_to_join.player_capacity {
                                    // Start the game soon..
                                    println!("lobby {} is starting", lobby_to_join.lobby_id);
                                    lobby_to_join.status = LobbyStatus::StartingSoon;
                                    lobby_to_join.next_starting_time =
                                        Utc::now().timestamp() + DELAY_FOR_GAMESTART_SEC;
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
                                            .read()
                                            .expect("failed to lock lobby chat")
                                            .messages
                                            .clone()
                                            .into_iter()
                                            .skip(
                                                state.lobbies[join_lobby_id]
                                                    .read()
                                                    .expect("failed to lock lobby chat")
                                                    .messages
                                                    .len()
                                                    .saturating_sub(DISPLAY_N_LAST_MESSAGES),
                                            )
                                            .take(DISPLAY_N_LAST_MESSAGES)
                                            .collect::<Vec<ChatMessage>>(),
                                    ))
                                    .expect("lobby chat sync failed");

                                drop(players); // unlock players because we are trying to lock it in global_lobby_update
                                global_lobbies_update(state.clone());
                            }
                        }
                        ClientCommand::Ping => {
                            state
                                .players
                                .read()
                                .expect("couldnt lock players")
                                .get(&player_uuid)
                                .expect("failed to get playername")
                                .personal_tx
                                .send(WsMessageToClient::Pong)
                                .expect("failed to pong player");
                        }
                        ClientCommand::SendGlobalMessage(message) => {
                            state
                                .global_chat_messages
                                .write()
                                .expect("failed to lock global chat")
                                .push(ChatMessage {
                                    poster: player_name.clone(),
                                    message: message.clone(),
                                });
                            global_chat_new_message(state.clone(), message, player_name.clone());
                        }
                        ClientCommand::SendLobbyMessage(message) => {
                            if let Some(lobby_player) = state
                                .players
                                .read()
                                .expect("couldnt lock players")
                                .get(&player_uuid)
                                .expect("couldnt find playername")
                                .playing_in_lobby
                            {
                                // can only send messages in the lobby youre in
                                state.lobbies[lobby_player]
                                    .write()
                                    .expect("failed ot lock lobby")
                                    .messages
                                    .push(ChatMessage {
                                        poster: player_name.clone(),
                                        message: message.clone(),
                                    });
                                state.lobbies[lobby_player]
                                    .read()
                                    .unwrap()
                                    .lobby_broadcast
                                    .send(WsMessageToClient::LobbyChatNewMessage(ChatMessage {
                                        poster: player_name.clone(),
                                        message,
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
        connected_players: state
            .players
            .read()
            .expect("failed to read")
            .values()
            .map(|player| (player.name.clone(), player.playing_in_lobby))
            .collect(),
        lobbies: vec![],
    };

    for lob in state.lobbies.iter() {
        let lobby = lob.read().expect("failed to lock lobby");
        update.lobbies.push(LobbyGeneralUpdate {
            player_capacity: lobby.player_capacity,
            player_names: lobby.players.values().cloned().collect(),
            status: lobby.status,
            next_starting_time: lobby.next_starting_time,
        });
    }
    state
        .global_broadcast
        .send(WsMessageToClient::LobbiesUpdate(update))
        .expect("global lobbies update failed");
}

fn global_chat_sync(
    perso_tx: tokio::sync::mpsc::UnboundedSender<WsMessageToClient>,
    state: Arc<configs::app_state::AppState>,
) {
    perso_tx
        .send(WsMessageToClient::GlobalChatSync(
            state
                .global_chat_messages
                .read()
                .expect("failed to lock global chat")
                .clone()
                .into_iter()
                .skip(
                    state
                        .global_chat_messages
                        .read()
                        .expect("failed to lock global chat")
                        .len()
                        .saturating_sub(DISPLAY_N_LAST_MESSAGES),
                )
                .take(DISPLAY_N_LAST_MESSAGES)
                .collect::<Vec<ChatMessage>>(),
        ))
        .expect("global chat sync failed");
}
fn global_chat_new_message(
    // todo : review usefullness of this function
    state: Arc<configs::app_state::AppState>,
    message: String,
    poster: String,
) {
    state
        .global_broadcast
        .send(WsMessageToClient::GlobalChatNewMessage(ChatMessage {
            poster,
            message,
        }))
        .expect("global chat new message failed");
}
