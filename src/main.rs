mod configs;
mod constants;
mod errors;
mod service_layer;
mod utilities;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Path, State},
    http,
    http::{HeaderValue, Method, StatusCode},
    response::IntoResponse,
    routing::get,
    routing::post,
    routing::put,
    Router,
};
use serde::Serialize;
use serde_json;
use tower_http::cors::CorsLayer;

use axum_extra::TypedHeader;
use std::{collections::HashMap, io::Error, str::FromStr};
use tokio::{
    sync::{
        broadcast::{self, Receiver},
        mpsc,
    },
    task::JoinError,
};

use std::borrow::Cow;
use std::ops::ControlFlow;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use std::{net::SocketAddr, path::PathBuf};
use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

//allows to extract the IP of connecting user
use axum::extract::connect_info::ConnectInfo;
use axum::extract::ws::CloseFrame;
//allows to split the websocket stream into separate TX and RX branches
use futures_util::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use rand::Rng;
use tower_http::cors::Any;

use crate::constants::constants::NB_LOBBIES;

#[derive(Debug)]
enum Command {
    JoinLobby(usize),
    Move(String),
    Ping,
}

impl FromStr for Command {
    type Err = ();
    fn from_str(msg: &str) -> Result<Command, Self::Err> {
        let commands: Vec<&str> = msg.splitn(2, ' ').collect();
        if commands.len() != 2 {
            // todo : recheck this, not always==2
            return Err(());
        }
        match commands[0] {
            "/joinlobby" => Ok(Command::JoinLobby(
                commands[1]
                    .parse::<usize>()
                    .expect("couldnt parse lobby id string to usize"),
            )),
            "/move" => Ok(Command::Move(commands[1].to_string())),
            "/ping" => Ok(Command::Ping),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
enum ClientMessage {
    Text(String), // todo : remove and use specific enum
    Pong,
    NbConnectedUsers(usize),
    JoinLobby(usize),
    LobbiesUpdate(LobbiesUpdate), // Test { x: i32 },
}
#[derive(Debug, Clone, Serialize)]
struct LobbiesUpdate {
    lobbies: Vec<LobbyUpdate>, // users_per_lobby: [usize; NB_LOBBIES],
                               // users_per_lobby: [usize; NB_LOBBIES],
}
#[derive(Debug, Clone, Serialize)]
struct LobbyUpdate {
    nb_users: usize,
    status: String,
}

impl ClientMessage {
    fn to_string_message(&self) -> Message {
        match &self {
            ClientMessage::Pong => Message::Text("/pong".to_string()),
            ClientMessage::NbConnectedUsers(nb_users) => {
                Message::Text(format!("{}{}", "/nbUsersConnected ", nb_users))
            }
            ClientMessage::JoinLobby(lobby_id) => Message::Text("TODO?".to_string()),
            ClientMessage::LobbiesUpdate(update) => Message::Text(format!(
                "{}{}",
                "/lobbiesUpdate ",
                serde_json::to_string(update).expect("failed to jsonize lobbies update")
            )),
            ClientMessage::Text(str) => Message::Text(str.to_string()),
        }
    }
}

#[tokio::main]
async fn main() {
    println!("Hello, world!");

    let app_state = configs::app_state::AppState::new();
    let config = configs::config::Config::new();

    let app = Router::new()
        .route("/ws/:player_uuid", get(websocket_connection))
        .route(
            "/players/uuid",
            get(service_layer::player_service::request_uuid),
        )
        .route(
            "/players/:uuid",
            put(service_layer::player_service::set_username),
        )
        .layer(
            CorsLayer::new()
                .allow_origin(
                    Any, // config
                        //     .wed_domains
                        //     .iter()
                        //     .map(|domain| {
                        //         domain
                        //             .parse::<HeaderValue>()
                        //             .expect("parse web domains into HeaderValue failed")
                        //     })
                        //     .collect::<Vec<HeaderValue>>(),
                )
                .allow_headers(vec![
                    http::header::CONTENT_TYPE,
                    http::header::AUTHORIZATION,
                    http::header::ACCEPT,
                    http::header::HeaderName::from_lowercase(b"trace").unwrap(),
                ])
                .allow_methods(vec![
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::DELETE,
                    Method::OPTIONS,
                ]),
        )
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(SocketAddr::from((config.ip, config.port)))
        .await
        .unwrap();

    axum::serve(listener, app).await.unwrap();
}

async fn websocket_connection(
    ws: WebSocketUpgrade,
    State(state): State<Arc<configs::app_state::AppState>>,
    Path(user_uuid): Path<String>,
) -> impl IntoResponse {
    println!("new connection {:?}", state);
    println!("new connection {:?}", user_uuid);
    // todo : check valid uuid
    ws.on_upgrade(|socket| websocket(user_uuid, socket, state))
}

async fn websocket(
    user_uuid: String,
    mut socket: WebSocket,
    state: Arc<configs::app_state::AppState>,
) {
    let (mut sender, mut receiver) = socket.split();
    println!("{:?}", state);
    // todo : return / timeout after x seconds
    let perso_tx = broadcast::channel(1).0;

    state
        .users
        .lock()
        .expect("couldnt lock users mutex")
        .insert(
            user_uuid.clone(),
            service_layer::player_service::Player {
                uuid: user_uuid.clone(),
                name: None,
                personal_tx: perso_tx.clone(),
                playing_in_lobby: None,
            },
        );

    let mut global_subscription = state.global_broadcast.subscribe();
    let mut personal_subscription = perso_tx.subscribe();
    let mut lobby_subscription: Receiver<ClientMessage> = broadcast::channel(1).1;
    let cloned_state = state.clone();
    let cloned_user_uuid = user_uuid.clone();
    let mut handle1 =
        tokio::spawn(async move { receive(&mut receiver, user_uuid.clone(), cloned_state).await });

    state.global_broadcast.send(ClientMessage::NbConnectedUsers(
        state.users.lock().expect("failed to lock users").len(),
    ));

    let mut upd: LobbiesUpdate = LobbiesUpdate { lobbies: vec![] };
    for lob in state.lobby.iter() {
        upd.lobbies.push(LobbyUpdate {
            nb_users: lob.lock().expect("failed to lock lobby").users.len(),
            status: "YOYO".to_string(),
        });
    }
    state
        .global_broadcast
        .send(ClientMessage::LobbiesUpdate(upd));
    let cloned_state = state.clone();
    let mut handle2 = tokio::spawn(async move {
        loop {
            // TODO : always send msg.to_string_message except in specific case
            tokio::select! {
                Ok(msg) = global_subscription.recv() => {
                    match msg {
                        ClientMessage::Text(str) => {
                            sender.send(Message::Text(str)).await.expect("globav tx send fail");
                        }
                        _ => sender.send(msg.to_string_message()).await.expect("failed to send to global sub")
                    };
                },
                Ok(msg) = personal_subscription.recv() => {
                    println!("sending personal merssage {:?}", msg);
                    match msg {
                        ClientMessage::Text(str) => {
                            sender.send(Message::Text(str)).await.expect("msg send fail");
                        }
                        ClientMessage::JoinLobby(lobby_id) => {
                            // todo : check bound lobby id
                            lobby_subscription = state.lobby[lobby_id].lock().unwrap().lobby_broadcast.subscribe();
                        },
                        _ => sender.send(msg.to_string_message()).await.expect("failed to send to personal sub")
                    };
                },
                Ok(msg) = lobby_subscription.recv() => {
                    match msg {
                        ClientMessage::Text(str) => {
                            sender.send(Message::Text(str)).await.expect("lobby message failed");
                        }
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

    if let Some(lobby_id) = cloned_state
        .users
        .lock()
        .expect("failed to lock users")
        .get(&cloned_user_uuid)
        .expect("failed to get user by uuid")
        .playing_in_lobby
    {
        cloned_state.lobby[lobby_id]
            .lock()
            .unwrap()
            .users
            .remove(&cloned_user_uuid);
    }

    cloned_state
        .users
        .lock()
        .expect("failed to lock users to remove disconnected")
        .remove(&cloned_user_uuid)
        .expect("failed to remove user from users list");
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
                                player.personal_tx.send(ClientMessage::JoinLobby(lobby_id));

                                // todo : this function is used in many places, extract it into a function
                                let mut upd: LobbiesUpdate = LobbiesUpdate { lobbies: vec![] };
                                for lob in state.lobby.iter() {
                                    upd.lobbies.push(LobbyUpdate {
                                        nb_users: lob
                                            .lock()
                                            .expect("failed to lock lobby")
                                            .users
                                            .len(),
                                        status: "YOYO".to_string(),
                                    });
                                }
                                state
                                    .global_broadcast
                                    .send(ClientMessage::LobbiesUpdate(upd));
                            }
                        }
                        Command::Move(position) => {
                            if let Some(lobby_user) = state
                                .users
                                .lock()
                                .expect("couldnt lock users")
                                .get(&user_uuid)
                                .expect("couldnt find username")
                                .playing_in_lobby
                            {
                                state.lobby[lobby_user]
                                    .lock()
                                    .unwrap()
                                    .lobby_broadcast
                                    .send(ClientMessage::Text(position));
                            }
                        }
                        Command::Ping => {
                            let wait: u64 = rand::thread_rng().gen_range(0..20);
                            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                            state
                                .users
                                .lock()
                                .expect("couldnt lock users")
                                .get(&user_uuid)
                                .expect("failed to get username")
                                .personal_tx
                                .send(ClientMessage::Pong);
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
