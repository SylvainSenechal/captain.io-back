use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct CreatePlayerRequest {
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RequestNewPlayerResponse {
    pub uuid: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateNameRequest {
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct IsValidPlayernameRequest {
    pub name: String,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct IsValidPlayernameResponse {
    pub is_valid: bool,
    pub reason: Option<String>,
}
