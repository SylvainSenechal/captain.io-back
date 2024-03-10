use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub port: u16,
    pub ip: [u8; 4],
    pub wed_domains: Vec<String>,
}

impl Config {
    pub fn new() -> Config {
        #[cfg(debug_assertions)]
        let filename = "src/configs/dev.toml";
        #[cfg(not(debug_assertions))]
        let filename = "src/configs/prod.toml";

        let file_content = fs::read_to_string(filename).expect("failed to read toml config");
        toml::from_str(&file_content).expect("failed to parse string file into toml")
    }
}
