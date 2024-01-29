use crate::configs;
use crate::configs::app_state::ChatMessage;
use crate::constants::constants::{DELAY_FOR_GAMESTART_SEC, NB_LOBBIES};
use crate::models::messages_from_clients::ClientCommand;
use crate::models::messages_to_clients::LobbyStatus;
use crate::models::{
    messages_to_clients::LobbiesGeneralUpdate, messages_to_clients::LobbyGeneralUpdate,
    messages_to_clients::WsMessageToClient,
};
use crate::service_layer::player_service;
use axum::extract::ws::{Message, WebSocket};
use chrono::Utc;
use futures_util::{
    sink::SinkExt,
    stream::{SplitStream, StreamExt},
};
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
    let perso_tx = broadcast::channel(100).0;

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
    let mut lobby_subscription: Receiver<WsMessageToClient> = broadcast::channel(100).1;
    let cloned_state = state.clone();
    let cloned_user_uuid = user_uuid.clone();
    let mut handle1 =
        tokio::spawn(async move { receive(&mut receiver, user_uuid.clone(), cloned_state).await });

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
                           println!("eeee 2 {}", e);
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
        state.lobbies[lobby_id]
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
    global_lobbies_update(state.clone());
}

async fn receive(
    receiver: &mut SplitStream<WebSocket>,
    user_uuid: String,
    state: Arc<configs::app_state::AppState>,
) {
    // todo : add disconnected for afk, spawn task waiter, reset to 0 after each message, if time > 100 : return err afk
    'rec_v_loop: while let Some(Ok(message)) = receiver.next().await {
        match message {
            Message::Text(msg) => {
                let command = msg.parse::<ClientCommand>();
                if let Ok(c) = command {
                    match c {
                        ClientCommand::JoinLobby(join_lobby_id) => {
                            println!("JOIN LOBBY {:?}", join_lobby_id);
                            if join_lobby_id < NB_LOBBIES {
                                let mut players = state.users.lock().expect("failed to lock users");
                                let mut lobby_to_join = state.lobbies[join_lobby_id]
                                    .lock()
                                    .expect("failed ot lock lobby");
                                if lobby_to_join.users.len() >= lobby_to_join.player_capacity {
                                    continue 'rec_v_loop;
                                }
                                if lobby_to_join.status != LobbyStatus::AwaitingUsers {
                                    continue 'rec_v_loop;
                                }
                                let player =
                                    players.get_mut(&user_uuid).expect("failed to get username");
                                if let Some(in_lobby) = player.playing_in_lobby {
                                    if in_lobby == join_lobby_id {
                                        // don't join a lobby you're already in
                                        continue 'rec_v_loop;
                                    }
                                    // remove from current lobby before joining the new one
                                    state.lobbies[in_lobby]
                                        .lock()
                                        .unwrap()
                                        .users
                                        .remove(&user_uuid.clone());
                                }
                                lobby_to_join.users.insert(user_uuid.clone());
                                player.playing_in_lobby = Some(join_lobby_id);
                                if lobby_to_join.users.len() == lobby_to_join.player_capacity {
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
                                .users
                                .lock()
                                .expect("couldnt lock users")
                                .get(&user_uuid)
                                .expect("failed to get username")
                                .personal_tx
                                .send(WsMessageToClient::Pong)
                                .expect("failed to pong user");
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
                            if let Some(lobby_user) = state
                                .users
                                .lock()
                                .expect("couldnt lock users")
                                .get(&user_uuid)
                                .expect("couldnt find username")
                                .playing_in_lobby
                            {
                                // can only send messages in the lobby youre in
                                state.lobbies[lobby_user]
                                    .lock()
                                    .expect("failed ot lock lobby")
                                    .messages
                                    .push(ChatMessage {
                                        message: message.clone(),
                                    });
                                state.lobbies[lobby_user]
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
            }
        }
    }
}

fn global_lobbies_update(state: Arc<configs::app_state::AppState>) {
    let mut update: LobbiesGeneralUpdate = LobbiesGeneralUpdate {
        total_users_connected: state.users.lock().expect("failed to lock users").len(),
        lobbies: vec![],
    };

    for lobby in state.lobbies.iter() {
        let lobby = lobby.lock().expect("failed to lock lobby");
        update.lobbies.push(LobbyGeneralUpdate {
            player_capacity: lobby.player_capacity,
            nb_connected: lobby.users.len(),
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
