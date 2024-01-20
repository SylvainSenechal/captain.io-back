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

use crate::service_layer::websocket_service::handle_websocket;

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
    ws.on_upgrade(|socket| handle_websocket(user_uuid, socket, state))
}
