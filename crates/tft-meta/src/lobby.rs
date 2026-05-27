//! Lobby operations — create lobby, start/stop matchmaking, leave lobby.
//!
//! Maps to LCU REST endpoints:
//! - POST /lol-lobby/v2/lobby  (create)
//! - POST /lol-lobby/v2/lobby/matchmaking/search  (start)
//! - DELETE /lol-lobby/v2/lobby/matchmaking/search  (stop)
//! - DELETE /lol-lobby/v2/lobby  (leave)

use anyhow::Result;
use serde_json::json;

use crate::lcu_client::LcuClient;

/// Create a TFT lobby with the given queue ID.
///
/// queue_id: 1090=normal, 1100=ranked, 1220=clockwork
pub fn create_lobby(client: &LcuClient, queue_id: u32) -> Result<()> {
    let body = json!({ "queueId": queue_id });
    let _: serde_json::Value = client.post("/lol-lobby/v2/lobby", Some(body))?;
    Ok(())
}

/// Start matchmaking search.
pub fn start_match(client: &LcuClient) -> Result<()> {
    let _: serde_json::Value = client.post("/lol-lobby/v2/lobby/matchmaking/search", None)?;
    Ok(())
}

/// Stop matchmaking search.
pub fn stop_match(client: &LcuClient) -> Result<()> {
    let status = client.delete("/lol-lobby/v2/lobby/matchmaking/search")?;
    if status == 404 {
        // Already left search, OK
        return Ok(());
    }
    Ok(())
}

/// Leave the current lobby.
pub fn leave_lobby(client: &LcuClient) -> Result<()> {
    let status = client.delete("/lol-lobby/v2/lobby")?;
    // 404 = lobby already gone, treat as success
    if status == 404 || status == 200 || status == 204 {
        return Ok(());
    }
    anyhow::bail!("leave_lobby returned status {}", status);
}

/// Check matchmaking search state.
///
/// Returns: "Invalid", "Searching", "Found"
pub fn check_search_state(client: &LcuClient) -> Result<String> {
    let resp: serde_json::Value = client.get("/lol-lobby/v2/lobby/matchmaking/search-state")?;
    let state = resp
        .get("searchState")
        .and_then(|v| v.as_str())
        .unwrap_or("Invalid")
        .to_string();
    Ok(state)
}

/// Get current gameflow phase as a string.
pub fn get_gameflow_phase(client: &LcuClient) -> Result<String> {
    let phase: String = client.get("/lol-gameflow/v1/gameflow-phase")?;
    Ok(phase)
}

/// Accept the ready-check.
pub fn accept_match(client: &LcuClient) -> Result<()> {
    let status = client.post_no_body("/lol-matchmaking/v1/ready-check/accept")?;
    if status == 200 || status == 204 {
        return Ok(());
    }
    anyhow::bail!("accept_match returned status {}", status);
}

/// Decline the ready-check.
pub fn decline_match(client: &LcuClient) -> Result<()> {
    let _status = client.post_no_body("/lol-matchmaking/v1/ready-check/decline")?;
    Ok(())
}

/// Quit game (early-exit from in-game).
pub fn quit_game(client: &LcuClient) -> Result<()> {
    let _status = client.post_no_body("/lol-gameflow/v1/early-exit")?;
    Ok(())
}
