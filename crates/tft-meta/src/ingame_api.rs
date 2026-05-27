//! In-game API client — polls 127.0.0.1:2999 for game-loaded status.
//!
//! The game exposes a REST API on port 2999 once the game client has loaded.
//! We use this to detect when the game is actually running (not just InProgress in LCU).

use anyhow::Result;
use reqwest::blocking::Client;
use std::time::Duration;

/// In-game API port (Riot fixed).
const IN_GAME_API_PORT: u16 = 2999;

/// In-game API client.
pub struct InGameApi {
    client: Client,
}

impl InGameApi {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(2))
            .no_proxy()
            .build()?;
        Ok(Self { client })
    }

    /// Check if the game has loaded by hitting the allgamedata endpoint.
    ///
    /// Returns true if the endpoint responds with 200.
    pub fn is_game_loaded(&self) -> bool {
        let url = format!(
            "https://127.0.0.1:{}/liveclientdata/allgamedata",
            IN_GAME_API_PORT
        );
        self.client
            .get(&url)
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Get all game data (returns raw JSON).
    pub fn get_all_game_data(&self) -> Result<serde_json::Value> {
        let url = format!(
            "https://127.0.0.1:{}/liveclientdata/allgamedata",
            IN_GAME_API_PORT
        );
        let resp = self.client.get(&url).send()?;
        Ok(resp.json()?)
    }
}
