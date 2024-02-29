use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::configs::app_state::AppState;
use crate::custom_errors::sqlite_errors::{map_sqlite_error, SqliteError};
use crate::requests::requests;

#[derive(Serialize, Deserialize, Debug)]
pub struct Player {
    pub uuid: String,
    pub name: String,
}

pub fn create_player(
    db: &Arc<AppState>,
    player: requests::CreatePlayerRequest,
) -> Result<String, SqliteError> {
    let binding = db.connection.get().unwrap();
    let mut statement = binding
        .prepare_cached("INSERT INTO Players (uuid, name) VALUES (?, ?)")
        .map_err(map_sqlite_error)?;
    let uuid = Uuid::now_v7().to_string();
    statement
        .execute(params![uuid, player.name])
        .map_err(map_sqlite_error)?;

    Ok(uuid)
}

pub fn update_playername(
    db: &Arc<AppState>,
    player_uuid: String,
    new_name: String,
) -> Result<(), SqliteError> {
    let binding = db.connection.get().unwrap();
    let mut statement = binding
        .prepare_cached("Update Players set name = ? where uuid = ?")
        .map_err(map_sqlite_error)?;
    let uuid = Uuid::now_v7().to_string();
    statement
        .execute(params![new_name, player_uuid])
        .map_err(map_sqlite_error)?;
    Ok(())
}

pub fn get_player_by_name(db: &Arc<AppState>, name: String) -> Result<Player, SqliteError> {
    let binding = db.connection.get().unwrap();
    let mut statement = binding
        .prepare_cached("SELECT * FROM Players WHERE name = ? LIMIT 1")
        .map_err(map_sqlite_error)?;

    statement
        .query_row(params![name], |row| {
            Ok(Player {
                uuid: row.get("uuid")?,
                name: row.get("name")?,
            })
        })
        .map_err(map_sqlite_error)
}

pub fn get_player_by_uuid(db: &Arc<AppState>, uuid: String) -> Result<Player, SqliteError> {
    let binding = db.connection.get().unwrap();
    let mut statement = binding
        .prepare_cached("SELECT * FROM Players WHERE uuid = ? LIMIT 1")
        .map_err(map_sqlite_error)?;

    statement
        .query_row(params![uuid], |row| {
            Ok(Player {
                uuid: row.get("uuid")?,
                name: row.get("name")?,
            })
        })
        .map_err(map_sqlite_error)
}
