//! Ready-check polling — poll the LCU ready-check endpoint.
//!
//! Maps to: GET /lol-matchmaking/v1/ready-check


use serde::Deserialize;

use crate::lcu_client::LcuClient;

/// Ready-check state from LCU.
#[derive(Debug, Clone, Deserialize)]
pub struct ReadyCheckData {
    #[serde(default)]
    pub state: String,
    #[serde(default, rename = "playerResponse")]
    pub player_response: String,
    #[serde(default)]
    pub timer: f64,
    #[serde(default, rename = "dodgeWarning")]
    pub dodge_warning: String,
}

/// Poll the ready-check endpoint.
///
/// Returns None if no ready-check is active (404 or empty).
pub fn poll_ready_check(client: &LcuClient) -> Option<ReadyCheckData> {
    match client.get::<ReadyCheckData>("/lol-matchmaking/v1/ready-check") {
        Ok(data) => Some(data),
        Err(_) => None,
    }
}
