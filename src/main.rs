mod configs;
mod constants;
mod errors;
mod models;
mod service_layer;
mod utilities;

use axum::{
    extract::ws::WebSocketUpgrade,
    extract::{Path, State},
    http,
    http::Method,
    response::IntoResponse,
    routing::get,
    routing::post,
    routing::put,
    Router,
};
use tower_http::cors::CorsLayer;

use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::Any;

use crate::{
    models::messages_to_clients::{LobbyStatus, WsMessageToClient},
    service_layer::websocket_service::handle_websocket,
};

// todo : - test send message to an empty broadcaster (like early before the webserver is listening)
//        - + be careful when lobby is empty, send will fail, deal with it
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
                    // todo : fix this Any
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
        .with_state(app_state.clone());

    let listener = tokio::net::TcpListener::bind(SocketAddr::from((config.ip, config.port)))
        .await
        .unwrap();

    let game_loop_handler = tokio::spawn(async { game_loop(app_state).await });

    axum::serve(listener, app).await.unwrap();
}

use tokio::time::{sleep, Duration};
async fn game_loop(state: Arc<configs::app_state::AppState>) {
    // todo : use interval instead of sleep https://docs.rs/tokio/latest/tokio/time/fn.interval.html
    // be careful if the game loop is longer than the interval
    let mut i = 0;
    loop {
        println! {"loop {}", i};
        i = i + 1;
        sleep(Duration::from_millis(1000)).await;
        for mutex_lobby in state.lobbies.iter() {
            let lobby = mutex_lobby.lock().expect("failed to lock lobby");
            // todo starting soon + time ok
            if lobby.status == LobbyStatus::StartingSoon {
                println! {"lobby starting {:?}", lobby};
                println! {"lobby starting {:?}", lobby};
                lobby
                    .lobby_broadcast
                    .send(WsMessageToClient::GameStarted(lobby.lobby_id))
                    .expect("failed to notify game started");
            }
        }
    }
}

async fn websocket_connection(
    ws: WebSocketUpgrade,
    State(state): State<Arc<configs::app_state::AppState>>,
    Path(user_uuid): Path<String>,
) -> impl IntoResponse {
    println!("new connection {:?}", state);
    println!("new connection {:?}", user_uuid);
    // todo : check valid uuid
    ws.on_upgrade(|socket| handle_websocket(user_uuid, socket, state))
}
