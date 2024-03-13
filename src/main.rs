mod configs;
mod constants;
mod custom_errors;
mod data_access_layer;
mod models;
mod requests;
mod service_layer;
mod utilities;

use axum::{
    extract::{ws::WebSocketUpgrade, Path, State},
    http::{self, HeaderValue, Method, Response},
    response::IntoResponse,
    routing::{get, post, put},
    Router,
};
use tower_http::cors::CorsLayer;

use std::net::SocketAddr;
use std::sync::Arc;

use crate::{data_access_layer::player_dal, service_layer::websocket_service::handle_websocket};

// todo : reference instead of .clone()
// todo : surrender
// todo : games history
// todo : manual queue pointer update
#[tokio::main]
async fn main() {
    println!("Hello, world!");

    let app_state = configs::app_state::AppState::new();
    let config = configs::config::Config::new();

    let app = Router::new()
        .route("/ws/:player_uuid", get(websocket_connection))
        .route(
            "/players/new",
            get(service_layer::player_service::request_new_player),
        )
        .route(
            "/players/name/random",
            get(service_layer::player_service::get_random_name),
        )
        .route(
            "/players/name/is_valid",
            post(service_layer::player_service::is_valid_playername),
        )
        .route(
            "/players/:uuid",
            put(service_layer::player_service::set_playername),
        )
        .layer(
            CorsLayer::new()
                .allow_origin(
                    config
                        .wed_domains
                        .iter()
                        .map(|domain| {
                            domain
                                .parse::<HeaderValue>()
                                .expect("parse web domains into HeaderValue failed")
                        })
                        .collect::<Vec<HeaderValue>>(),
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

    tokio::spawn(async { service_layer::game_service::game_loop(app_state).await });
    axum::serve(listener, app).await.unwrap();
}

async fn websocket_connection(
    ws: WebSocketUpgrade,
    State(state): State<Arc<configs::app_state::AppState>>,
    Path(player_uuid): Path<String>,
) -> impl IntoResponse {
    println!("new connection {:?}", player_uuid);

    let player_in_db = player_dal::get_player_by_uuid(&state, player_uuid.clone());
    if let Err(err) = player_in_db {
        println!(
            "websocket_connection, can't find player with uuid={}, err={:?} ",
            player_uuid, err
        );
        return Response::default();
    }

    let player = player_in_db.unwrap();
    if state
        .players
        .read()
        .expect("failed to read players")
        .get(&player.uuid)
        .is_some()
    {
        println!(
            "websocket_connection, player uuid={} already connected",
            player_uuid
        );
        return Response::default();
    }

    ws.on_upgrade(|socket| handle_websocket(player, socket, state))
}
