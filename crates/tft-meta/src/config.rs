//! Meta-FSM configuration.

use tft_executor::lcu_gate::MetaMode;

/// Configuration for the meta-game FSM.
#[derive(Debug, Clone)]
pub struct MetaConfig {
    /// TFT queue ID (1090 = normal, 1100 = ranked, 1220 = clockwork)
    pub queue_id: u32,
    /// Lobby create retry count
    pub max_create_lobby_retries: u32,
    /// Lobby create retry delay (ms)
    pub create_lobby_retry_delay_ms: u64,
    /// Start match retry count
    pub max_start_match_retries: u32,
    /// Start match retry delay (ms)
    pub start_match_retry_delay_ms: u64,
    /// Ready-check poll interval (ms)
    pub ready_check_poll_ms: u64,
    /// 2999 poll interval (ms)
    pub ingame_api_poll_ms: u64,
    /// 2999 timeout (ms) — how long to wait for game to load
    pub ingame_api_timeout_ms: u64,
    /// Queue timeout (ms) — how long to wait for match before re-queuing
    pub queue_timeout_ms: u64,
    /// LCU lockfile path
    pub lockfile_path: String,
    /// Meta mode (lcu or manual)
    pub meta_mode: MetaMode,
}

impl Default for MetaConfig {
    fn default() -> Self {
        Self {
            queue_id: 1090, // TFT normal
            max_create_lobby_retries: 3,
            create_lobby_retry_delay_ms: 1000,
            max_start_match_retries: 5,
            start_match_retry_delay_ms: 500,
            ready_check_poll_ms: 500,
            ingame_api_poll_ms: 500,
            ingame_api_timeout_ms: 120_000, // 2 minutes
            queue_timeout_ms: 300_000,      // 5 minutes
            lockfile_path: tft_executor::lcu_gate::DEFAULT_LOCKFILE_PATH.to_string(),
            meta_mode: MetaMode::Manual,
        }
    }
}
