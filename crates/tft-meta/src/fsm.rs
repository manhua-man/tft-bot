//! Meta-FSM — Lobby → Accept → Loading → Running state machine.
//!
//! This is the core loop that automates the meta-game flow.
//!
//! State transitions:
//! ```text
//! Start → Lobby → LobbyWait → GameLoading → GameRunning → (callback) → End
//!                ↑    ↓                         ↑
//!                └────┘ (retry)                 └── (next game) → Lobby
//! ```

use anyhow::Result;
use tft_executor::lcu_gate::MetaMode;

use crate::config::MetaConfig;
use crate::ingame_api::InGameApi;
use crate::lcu_client::LcuClient;
use crate::lobby;
use crate::ready_check;

/// Meta-FSM states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaState {
    /// Initial state — decide based on meta_mode
    Start,
    /// Creating lobby + starting matchmaking
    Lobby,
    /// Waiting for ready-check / match found
    LobbyWait,
    /// Game is loading — waiting for 2999 API
    GameLoading,
    /// Game is running — hand off to in-game loop
    GameRunning,
    /// Match ended — return to lobby or exit
    GameEnded,
    /// Terminal state
    End,
}

/// Outcome of a single game run (returned by the in-game callback).
#[derive(Debug, Clone)]
pub struct GameOutcome {
    pub steps: usize,
    pub total_reward: f32,
    pub placement: Option<f32>,
    pub redline_reason: Option<String>,
    pub redline_triggered: bool,
}

/// Meta-FSM that drives the lobby → game → lobby loop.
pub struct MetaFsm {
    config: MetaConfig,
    state: MetaState,
    /// Current game number (1-indexed)
    pub game_number: u32,
    /// Total games to play (0 = unlimited)
    pub max_games: u32,
}

impl MetaFsm {
    pub fn new(config: MetaConfig) -> Self {
        Self {
            state: MetaState::Start,
            config,
            game_number: 0,
            max_games: 0,
        }
    }

    pub fn state(&self) -> MetaState {
        self.state
    }

    /// Run the meta loop. The `run_game` callback is called when the game is running.
    ///
    /// The callback receives a `&LcuClient` (if LCU mode) and should return a `GameOutcome`.
    pub fn run<F>(&mut self, mut run_game: F) -> Result<Vec<GameOutcome>>
    where
        F: FnMut(Option<&LcuClient>) -> Result<GameOutcome>,
    {
        let mut outcomes = Vec::new();

        loop {
            match self.state {
                MetaState::Start => match self.config.meta_mode {
                    MetaMode::Lcu => {
                        eprintln!("[meta] Mode: LCU, queue_id={}", self.config.queue_id);
                        self.state = MetaState::Lobby;
                    }
                    MetaMode::Manual => {
                        eprintln!("[meta] Mode: Manual — waiting for game window detection");
                        self.state = MetaState::GameLoading;
                    }
                },

                MetaState::Lobby => {
                    match self.run_lobby() {
                        Ok(()) => {
                            self.state = MetaState::LobbyWait;
                        }
                        Err(e) => {
                            eprintln!("[meta] Lobby failed: {e}");
                            // Retry from Lobby state
                            std::thread::sleep(std::time::Duration::from_millis(
                                self.config.create_lobby_retry_delay_ms,
                            ));
                        }
                    }
                }

                MetaState::LobbyWait => match self.run_lobby_wait() {
                    Ok(()) => {
                        self.state = MetaState::GameLoading;
                    }
                    Err(e) => {
                        eprintln!("[meta] LobbyWait failed: {e}");
                        self.state = MetaState::Lobby;
                    }
                },

                MetaState::GameLoading => {
                    match self.run_game_loading() {
                        Ok(()) => {
                            self.state = MetaState::GameRunning;
                        }
                        Err(e) => {
                            eprintln!("[meta] GameLoading failed: {e}");
                            // If in manual mode, keep waiting
                            if self.config.meta_mode == MetaMode::Manual {
                                std::thread::sleep(std::time::Duration::from_secs(2));
                            } else {
                                self.state = MetaState::Lobby;
                            }
                        }
                    }
                }

                MetaState::GameRunning => {
                    self.game_number += 1;
                    eprintln!("[meta] === Game {} starting ===", self.game_number);

                    let lcu = if self.config.meta_mode == MetaMode::Lcu {
                        // Create LCU client for game-end operations
                        let lf =
                            tft_executor::lcu_gate::read_lockfile(&self.config.lockfile_path).ok();
                        lf.as_ref().and_then(|lf| LcuClient::from_lockfile(lf).ok())
                    } else {
                        None
                    };

                    let outcome = run_game(lcu.as_ref())?;
                    eprintln!(
                        "[meta] Game {} done: {} steps, reward={:.2}, placement={:?}",
                        self.game_number, outcome.steps, outcome.total_reward, outcome.placement
                    );
                    outcomes.push(outcome);

                    // Check if we should continue
                    if self.max_games > 0 && self.game_number >= self.max_games {
                        eprintln!("[meta] Reached max_games={}", self.max_games);
                        self.state = MetaState::End;
                    } else {
                        self.state = MetaState::GameEnded;
                    }
                }

                MetaState::GameEnded => {
                    eprintln!("[meta] Game ended, waiting for stats...");
                    // Poll LCU for WaitingForStats/EndOfGame
                    if let Ok(lf) =
                        tft_executor::lcu_gate::read_lockfile(&self.config.lockfile_path)
                    {
                        if let Ok(client) = LcuClient::from_lockfile(&lf) {
                            // Wait for end-of-game phase (up to 30s)
                            let start = std::time::Instant::now();
                            loop {
                                if start.elapsed().as_secs() > 30 {
                                    eprintln!("[meta] Timeout waiting for end phase, force exit");
                                    break;
                                }
                                if let Ok(phase) = lobby::get_gameflow_phase(&client) {
                                    match phase.as_str() {
                                        "WaitingForStats" | "EndOfGame" | "PreEndOfGame" => {
                                            eprintln!("[meta] Phase: {phase}, exiting game");
                                            break;
                                        }
                                        "None" | "Lobby" => {
                                            eprintln!("[meta] Already back in lobby");
                                            break;
                                        }
                                        _ => {
                                            std::thread::sleep(std::time::Duration::from_secs(1));
                                        }
                                    }
                                } else {
                                    break;
                                }
                            }
                            let _ = lobby::quit_game(&client);
                            // Wait for lobby to be ready
                            std::thread::sleep(std::time::Duration::from_secs(3));
                        }
                    }
                    self.state = MetaState::Lobby;
                }

                MetaState::End => {
                    eprintln!("[meta] FSM ended after {} games", self.game_number);
                    break;
                }
            }
        }

        Ok(outcomes)
    }

    /// Run the lobby phase: create lobby + start matchmaking.
    fn run_lobby(&self) -> Result<()> {
        let lf = tft_executor::lcu_gate::read_lockfile(&self.config.lockfile_path)?;
        let client = LcuClient::from_lockfile(&lf)?;

        // Create lobby with retries
        let mut last_err = None;
        for attempt in 1..=self.config.max_create_lobby_retries {
            match lobby::create_lobby(&client, self.config.queue_id) {
                Ok(()) => {
                    eprintln!("[meta] Lobby created (attempt {attempt})");
                    std::thread::sleep(std::time::Duration::from_millis(500));

                    // Start matchmaking with retries
                    for m_attempt in 1..=self.config.max_start_match_retries {
                        match lobby::start_match(&client) {
                            Ok(()) => {
                                eprintln!("[meta] Matchmaking started (attempt {m_attempt})");
                                return Ok(());
                            }
                            Err(e) => {
                                let msg = e.to_string();
                                // 404/423 = already in game
                                if msg.contains("404") || msg.contains("423") {
                                    eprintln!("[meta] Already in match (status in error msg)");
                                    return Ok(());
                                }
                                eprintln!("[meta] start_match failed (attempt {m_attempt}): {msg}");
                                std::thread::sleep(std::time::Duration::from_millis(
                                    self.config.start_match_retry_delay_ms,
                                ));
                            }
                        }
                    }
                    last_err = Some(anyhow::anyhow!("start_match retries exhausted"));
                    break;
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("404") || msg.contains("423") {
                        eprintln!("[meta] Lobby already exists or locked, trying start_match");
                        // Try to start match directly
                        match lobby::start_match(&client) {
                            Ok(()) => return Ok(()),
                            Err(_) => {}
                        }
                    }
                    eprintln!("[meta] create_lobby failed (attempt {attempt}): {msg}");
                    last_err = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(
                        self.config.create_lobby_retry_delay_ms,
                    ));
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("lobby phase failed")))
    }

    /// Run the lobby-wait phase: poll ready-check, auto-accept, wait for InProgress.
    fn run_lobby_wait(&self) -> Result<()> {
        let lf = tft_executor::lcu_gate::read_lockfile(&self.config.lockfile_path)?;
        let client = LcuClient::from_lockfile(&lf)?;

        let start = std::time::Instant::now();
        let mut last_accept = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(10))
            .unwrap_or_else(std::time::Instant::now);

        loop {
            // Check queue timeout
            if self.config.queue_timeout_ms > 0
                && start.elapsed().as_millis() > self.config.queue_timeout_ms as u128
            {
                eprintln!("[meta] Queue timeout, leaving lobby");
                let _ = lobby::leave_lobby(&client);
                anyhow::bail!("queue timeout");
            }

            // Check gameflow phase
            if let Ok(phase) = lobby::get_gameflow_phase(&client) {
                match phase.as_str() {
                    "InProgress" => {
                        eprintln!("[meta] Game is InProgress");
                        return Ok(());
                    }
                    "GameStart" | "ChampSelect" => {
                        eprintln!("[meta] Game starting ({phase})");
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        continue;
                    }
                    "TerminatedInError" => {
                        anyhow::bail!("game terminated in error");
                    }
                    _ => {}
                }
            }

            // Poll ready-check and auto-accept
            if let Some(rc) = ready_check::poll_ready_check(&client) {
                if rc.state == "InProgress" && last_accept.elapsed().as_millis() > 1000 {
                    eprintln!("[meta] Ready-check found, accepting...");
                    match lobby::accept_match(&client) {
                        Ok(()) => {
                            eprintln!("[meta] Match accepted");
                            last_accept = std::time::Instant::now();
                        }
                        Err(e) => {
                            eprintln!("[meta] Accept failed: {e}");
                        }
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(
                self.config.ready_check_poll_ms,
            ));
        }
    }

    /// Run the game-loading phase: poll 2999 until game loads.
    fn run_game_loading(&self) -> Result<()> {
        let api = InGameApi::new()?;
        let start = std::time::Instant::now();

        eprintln!("[meta] Waiting for game to load (2999 API)...");

        loop {
            if self.config.ingame_api_timeout_ms > 0
                && start.elapsed().as_millis() > self.config.ingame_api_timeout_ms as u128
            {
                anyhow::bail!("game load timeout (2999 API not responding)");
            }

            if api.is_game_loaded() {
                eprintln!("[meta] Game loaded (2999 OK)");
                return Ok(());
            }

            std::thread::sleep(std::time::Duration::from_millis(
                self.config.ingame_api_poll_ms,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_state_transitions() {
        let config = MetaConfig {
            meta_mode: MetaMode::Manual,
            ..Default::default()
        };
        let mut fsm = MetaFsm::new(config);
        assert_eq!(fsm.state(), MetaState::Start);
    }

    #[test]
    fn meta_state_display() {
        assert_eq!(format!("{:?}", MetaState::Start), "Start");
        assert_eq!(format!("{:?}", MetaState::GameRunning), "GameRunning");
    }
}
