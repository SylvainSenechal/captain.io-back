use crate::configs;
use crate::configs::app_state::ChatMessage;
use crate::constants::constants::NB_LOBBIES;
use crate::models::messages_from_clients::Command;
use crate::models::{
    messages_to_clients::LobbiesUpdate, messages_to_clients::LobbyUpdate,
    messages_to_clients::WsMessageToClient,
};
use crate::service_layer::player_service;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Path, State},
    http,
    http::Method,
    response::IntoResponse,
    routing::get,
    routing::post,
    routing::put,
    Router,
};
use futures_util::{
    sink::SinkExt,
    stream::{SplitStream, StreamExt},
};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast::{self, Receiver};

pub async fn handle_websocket(
    user_uuid: String,
    socket: WebSocket,
    state: Arc<configs::app_state::AppState>,
) {
    let (mut sender, mut receiver) = socket.split();
    println!("{:?}", state);
    // todo : return / timeout after x seconds
    let perso_tx = broadcast::channel(10).0;

    state
        .users
        .lock()
        .expect("couldnt lock users mutex")
        .insert(
            user_uuid.clone(),
            player_service::Player {
                uuid: user_uuid.clone(),
                name: None,
                personal_tx: perso_tx.clone(),
                playing_in_lobby: None,
            },
        );

    let mut global_subscription = state.global_broadcast.subscribe();
    let mut personal_subscription = perso_tx.subscribe();
    let mut lobby_subscription: Receiver<WsMessageToClient> = broadcast::channel(10).1;
    let cloned_state = state.clone();
    let cloned_user_uuid = user_uuid.clone();
    let mut handle1 =
        tokio::spawn(async move { receive(&mut receiver, user_uuid.clone(), cloned_state).await });

    global_lobby_update(state.clone());
    global_chat_sync(state.clone());
    global_chat_new_message(state.clone(), "new message heheh".to_string());

    let cloned_state = state.clone();
    let mut handle2 = tokio::spawn(async move {
        loop {
            // TODO : always send msg.to_string_message except in specific case
            tokio::select! {
                Ok(msg) = global_subscription.recv() => {
                    match msg {
                        _ => sender.send(msg.to_string_message()).await.expect("failed to send to global sub")
                    };
                },
                Ok(msg) = personal_subscription.recv() => {
                    println!("sending personal message {:?}", msg);
                    match msg {
                        WsMessageToClient::JoinLobby(lobby_id) => {
                            // todo : check bound lobby id
                            println!("sending personal join lobby, {:?}", msg.to_string_message());
                            lobby_subscription = cloned_state.lobby[lobby_id].lock().unwrap().lobby_broadcast.subscribe();
                            sender.send(msg.to_string_message()).await.expect("failed to send to global sub");
                        },
                        _ => sender.send(msg.to_string_message()).await.expect("failed to send to personal sub")
                    };
                },
                // TODO : handle ?
                // Err(e) = personal_subscription.recv() => {
                    // println!("eeee {}", e); => eeee channel lagged by 1
                // }
                // let res = personal_subscription.recv().await ?
                // match res ...
                Ok(msg) = lobby_subscription.recv() => {
                    println!("sending lobby message {:?}", msg);
                    match msg {
                        _ => sender.send(msg.to_string_message()).await.expect("failed to send to lobby sub")
                    };
                },
                else => break,
            }
        }
    });

    // Waiting while the user is connected
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

    // Logic after disconnected user
    if let Some(lobby_id) = state
        .users
        .lock()
        .expect("failed to lock users")
        .get(&cloned_user_uuid)
        .expect("failed to get user by uuid")
        .playing_in_lobby
    {
        state.lobby[lobby_id]
            .lock()
            .unwrap()
            .users
            .remove(&cloned_user_uuid);
    }

    state
        .users
        .lock()
        .expect("failed to lock users to remove disconnected")
        .remove(&cloned_user_uuid)
        .expect("failed to remove user from users list");
    global_lobby_update(state.clone());
}

async fn receive(
    receiver: &mut SplitStream<WebSocket>,
    user_uuid: String,
    state: Arc<configs::app_state::AppState>,
) {
    // todo : add disconnected for afk, spawn task waiter, reset to 0 after each message, if time > 100 : return err afk
    while let Some(Ok(message)) = receiver.next().await {
        match message {
            Message::Text(msg) => {
                let command = msg.parse::<Command>();
                if let Ok(c) = command {
                    match c {
                        Command::JoinLobby(lobby_id) => {
                            // todo : check if the user was in another lobby
                            // todo : check that the lobby is in waiting state
                            // todo : check lobby capacity
                            println!("JOIN LOBBY {:?}", lobby_id);
                            if lobby_id < NB_LOBBIES {
                                let mut players = state.users.lock().expect("failed to lock users");
                                let player =
                                    players.get_mut(&user_uuid).expect("failed to get username");
                                if let Some(current_lobby) = player.playing_in_lobby {
                                    state.lobby[current_lobby]
                                        .lock()
                                        .unwrap()
                                        .users
                                        .remove(&user_uuid.clone());
                                }
                                state.lobby[lobby_id]
                                    .lock()
                                    .unwrap()
                                    .users
                                    .insert(user_uuid.clone());

                                player.playing_in_lobby = Some(lobby_id);
                                player
                                    .personal_tx
                                    .send(WsMessageToClient::JoinLobby(lobby_id))
                                    .expect("failed to notify joined lobby");
                                player
                                    .personal_tx
                                    .send(WsMessageToClient::LobbyChatSync(
                                        state.lobby[lobby_id]
                                            .lock()
                                            .expect("failed to lock lobby")
                                            .messages
                                            .to_owned(),
                                    ))
                                    .expect("failed to notify current lobby chat");
                                drop(players); // unlock players because we are trying to lock it in global_lobby_update
                                global_lobby_update(state.clone());
                            }
                        }
                        Command::Ping => {
                            state
                                .users
                                .lock()
                                .expect("couldnt lock users")
                                .get(&user_uuid)
                                .expect("failed to get username")
                                .personal_tx
                                .send(WsMessageToClient::Pong)
                                .expect("failed to pong user");
                        }
                        Command::SendGlobalMessage(message) => {
                            println!("global message {}", message);
                            state
                                .global_chat_messages
                                .lock()
                                .expect("failed to lock global chat")
                                .push(ChatMessage {
                                    message: message.clone(),
                                });
                            println!("looo global message {:?}", state.global_chat_messages);
                            // global_chat_sync(state.clone());
                            global_chat_new_message(state.clone(), message);
                        }
                        Command::SendLobbyMessage(lobby_id, message) => {
                            println!("lobby id {}", lobby_id);
                            println!("message {}", message);
                            state.lobby[lobby_id]
                                .lock()
                                .expect("failed ot lock lobby")
                                .messages
                                .push(ChatMessage {
                                    message: message.clone(),
                                });

                            global_lobby_update(state.clone());
                            if let Some(lobby_user) = state
                                .users
                                .lock()
                                .expect("couldnt lock users")
                                .get(&user_uuid)
                                .expect("couldnt find username")
                                .playing_in_lobby
                            {
                                println!("playing in lobby {}", lobby_user);

                                if lobby_id == lobby_user {
                                    state.lobby[lobby_user]
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
            }
            _ => {
                println!("received unexpected message {:?}", message);
            }
        }
    }
}

fn global_lobby_update(state: Arc<configs::app_state::AppState>) {
    let mut update: LobbiesUpdate = LobbiesUpdate {
        total_users_connected: state.users.lock().expect("failed to lock users").len(),
        lobbies: vec![],
    };

    for lob in state.lobby.iter() {
        let lobby = lob.lock().expect("failed to lock lobby");
        update.lobbies.push(LobbyUpdate {
            nb_users: lobby.users.len(),
            status: lobby.status,
        });
    }
    state
        .global_broadcast
        .send(WsMessageToClient::LobbiesUpdate(update));
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
        ));
}
fn global_chat_new_message(state: Arc<configs::app_state::AppState>, message: String) {
    state
        .global_broadcast
        .send(WsMessageToClient::GlobalChatNewMessage(ChatMessage {
            message: message,
        }));
}
