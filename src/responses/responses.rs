use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct IsValidPlayernameResponse {
    pub is_valid: bool,
    pub reason: Option<String>,
}
