//! Phase detector — unified game phase detection using LCU or visual fallback.
//!
//! M4.1: Determines what phase the game is in (lobby, shop, combat, etc.)
//! so the agent knows which actions are legal.
//!
//! Two modes:
//! - **B (LCU available)**: Queries LCU gameflow-phase endpoint periodically
//! - **A (visual fallback)**: Uses OCR/keyword matching on screen regions

use crate::lcu_gate::{GamePhase, LcuGate, LcuProbeResult};
use crate::ocr::OcrEngine;
use crate::shop::ShopReader;
use crate::window::GameWindow;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Unified game phase for the agent's decision-making.
///
/// Maps both LCU phases and visual detection results into a common set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentPhase {
    /// Not in a game (lobby, queue, etc.)
    Idle,
    /// In champion/unit select (TFT: choosing starting items/carousel)
    ChampSelect,
    /// In the shop/planning phase (buy, sell, position)
    ShopPhase,
    /// Augment selection round (choose one of three augments)
    Augment,
    /// In combat (watching the fight)
    Combat,
    /// Post-combat (results shown, transitioning)
    PostCombat,
    /// Carousel/selection round
    Carousel,
    /// Game ended
    GameOver,
    /// Unknown or transitioning
    Unknown,
}

impl AgentPhase {
    /// Can the agent take shop-related actions (buy, sell, reroll)?
    pub fn can_shop(&self) -> bool {
        matches!(self, Self::ShopPhase)
    }

    /// Can the agent take board-related actions (move, position)?
    pub fn can_position(&self) -> bool {
        matches!(self, Self::ShopPhase | Self::PostCombat)
    }

    /// Can the agent choose an augment?
    pub fn can_choose_augment(&self) -> bool {
        matches!(self, Self::Augment)
    }

    /// Is the agent in an active game (not idle or game over)?
    pub fn is_in_game(&self) -> bool {
        !matches!(self, Self::Idle | Self::GameOver | Self::Unknown)
    }

    /// Map from LCU GamePhase to AgentPhase.
    pub fn from_lcu(phase: &GamePhase) -> Self {
        match phase {
            GamePhase::None | GamePhase::Lobby | GamePhase::Matchmaking
            | GamePhase::ReadyCheck => Self::Idle,
            GamePhase::ChampSelect => Self::ChampSelect,
            GamePhase::GameStart => Self::ShopPhase,
            GamePhase::InProgress => Self::ShopPhase, // best guess; visual refines
            GamePhase::WaitingForStats => Self::PostCombat,
            GamePhase::EndOfGame => Self::GameOver,
            GamePhase::Reconnect => Self::Unknown,
            GamePhase::Unknown(_) => Self::Unknown,
        }
    }
}

impl std::fmt::Display for AgentPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Configuration for phase detection.
#[derive(Debug, Clone)]
pub struct PhaseDetectorConfig {
    /// LCU lockfile path
    pub lockfile_path: String,
    /// How often to re-query LCU (ms)
    pub lcu_poll_interval_ms: u64,
    /// Minimum time between phase changes (debounce, ms)
    pub debounce_ms: u64,
}

impl Default for PhaseDetectorConfig {
    fn default() -> Self {
        Self {
            lockfile_path: crate::lcu_gate::DEFAULT_LOCKFILE_PATH.to_string(),
            lcu_poll_interval_ms: 2000,
            debounce_ms: 500,
        }
    }
}

/// Phase detector that uses LCU when available, visual fallback otherwise.
pub struct PhaseDetector {
    config: PhaseDetectorConfig,
    lcu_gate: Option<LcuGate>,
    current_phase: AgentPhase,
    last_lcu_poll: Instant,
    last_phase_change: Instant,
    /// Consecutive reads confirming the same phase (for debounce)
    stable_count: u32,
    pending_phase: AgentPhase,
}

impl PhaseDetector {
    /// Create a new phase detector. Probes LCU on creation.
    pub fn new(config: PhaseDetectorConfig) -> Self {
        let lcu_gate = LcuGate::probe(&config.lockfile_path);
        let has_lcu = lcu_gate.is_available();

        let initial = if has_lcu {
            if let Some(phase) = lcu_gate.phase() {
                AgentPhase::from_lcu(phase)
            } else {
                AgentPhase::Unknown
            }
        } else {
            AgentPhase::Unknown
        };

        Self {
            config,
            lcu_gate: if has_lcu { Some(lcu_gate) } else { None },
            current_phase: initial,
            last_lcu_poll: Instant::now(),
            last_phase_change: Instant::now(),
            stable_count: 1,
            pending_phase: initial,
        }
    }

    /// Create from a known LCU probe result (for testing).
    pub fn from_probe(probe: LcuProbeResult, config: PhaseDetectorConfig) -> Self {
        let has_lcu = probe.available;
        let lcu_gate = LcuGate::from_probe(probe);
        let initial = if has_lcu {
            if let Some(phase) = lcu_gate.phase() {
                AgentPhase::from_lcu(phase)
            } else {
                AgentPhase::Unknown
            }
        } else {
            AgentPhase::Unknown
        };

        Self {
            config,
            lcu_gate: if has_lcu { Some(lcu_gate) } else { None },
            current_phase: initial,
            last_lcu_poll: Instant::now(),
            last_phase_change: Instant::now(),
            stable_count: 1,
            pending_phase: initial,
        }
    }

    /// Is LCU available for phase detection?
    pub fn is_lcu_available(&self) -> bool {
        self.lcu_gate.is_some()
    }

    /// Get the current detected phase.
    pub fn current_phase(&self) -> AgentPhase {
        self.current_phase
    }

    /// Update phase from LCU (call periodically).
    ///
    /// Returns true if the phase changed.
    pub fn update_lcu(&mut self) -> bool {
        if self.lcu_gate.is_none() {
            return false;
        }

        // Rate-limit LCU polling
        if self.last_lcu_poll.elapsed()
            < Duration::from_millis(self.config.lcu_poll_interval_ms)
        {
            return false;
        }
        self.last_lcu_poll = Instant::now();

        // Re-probe to get fresh phase
        let fresh_gate = LcuGate::probe(&self.config.lockfile_path);
        let new_phase = fresh_gate.phase().map(AgentPhase::from_lcu);
        self.lcu_gate = Some(fresh_gate);

        if let Some(phase) = new_phase {
            self.apply_phase_change(phase)
        } else {
            false
        }
    }

    /// Update phase from visual detection (OCR keywords on screen).
    ///
    /// This is the fallback when LCU is unavailable.
    /// Looks for keywords in the shop region to determine if we're in shop phase.
    pub fn update_visual<E: OcrEngine>(&mut self, ocr: E, window: &GameWindow) -> bool {
        // Try to read the shop region
        let reader = ShopReader::new(ocr, crate::correction::OcrCorrectionDict::new());
        let slots = reader.read_shop(window).unwrap_or_default();

        // If any slot has non-empty text, we're likely in shop phase
        let has_shop_text = slots.iter().any(|s| !s.corrected_text.trim().is_empty());

        let new_phase = if has_shop_text {
            AgentPhase::ShopPhase
        } else {
            // No shop text — could be combat or idle
            // Without more sophisticated detection, keep current phase
            return false;
        };

        self.apply_phase_change(new_phase)
    }

    /// Set phase manually (for testing or external control).
    pub fn set_phase(&mut self, phase: AgentPhase) {
        self.current_phase = phase;
        self.last_phase_change = Instant::now();
        self.stable_count = 1;
        self.pending_phase = phase;
    }

    /// Apply a candidate phase change with debounce.
    ///
    /// The phase only changes if the same candidate is seen consecutively
    /// enough times to pass the debounce threshold.
    fn apply_phase_change(&mut self, candidate: AgentPhase) -> bool {
        if candidate == self.current_phase {
            self.stable_count = 1;
            return false;
        }

        if candidate == self.pending_phase {
            self.stable_count += 1;
        } else {
            self.pending_phase = candidate;
            self.stable_count = 1;
        }

        // Require 2 consecutive confirmations to change phase
        if self.stable_count >= 2
            && self.last_phase_change.elapsed() >= Duration::from_millis(self.config.debounce_ms)
        {
            self.current_phase = candidate;
            self.last_phase_change = Instant::now();
            self.stable_count = 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_phase_from_lcu() {
        assert_eq!(AgentPhase::from_lcu(&GamePhase::InProgress), AgentPhase::ShopPhase);
        assert_eq!(AgentPhase::from_lcu(&GamePhase::Lobby), AgentPhase::Idle);
        assert_eq!(AgentPhase::from_lcu(&GamePhase::ChampSelect), AgentPhase::ChampSelect);
        assert_eq!(AgentPhase::from_lcu(&GamePhase::EndOfGame), AgentPhase::GameOver);
    }

    #[test]
    fn agent_phase_can_shop() {
        assert!(AgentPhase::ShopPhase.can_shop());
        assert!(!AgentPhase::Combat.can_shop());
        assert!(!AgentPhase::Idle.can_shop());
    }

    #[test]
    fn agent_phase_can_position() {
        assert!(AgentPhase::ShopPhase.can_position());
        assert!(AgentPhase::PostCombat.can_position());
        assert!(!AgentPhase::Combat.can_position());
    }

    #[test]
    fn agent_phase_is_in_game() {
        assert!(AgentPhase::ShopPhase.is_in_game());
        assert!(AgentPhase::Combat.is_in_game());
        assert!(!AgentPhase::Idle.is_in_game());
        assert!(!AgentPhase::GameOver.is_in_game());
    }

    #[test]
    fn phase_detector_with_lcu_unavailable() {
        let config = PhaseDetectorConfig {
            lockfile_path: "/nonexistent".to_string(),
            ..Default::default()
        };
        let probe = LcuProbeResult {
            available: false,
            lockfile_path: "/nonexistent".to_string(),
            lockfile: None,
            phase: None,
            error: Some("no lockfile".to_string()),
        };
        let detector = PhaseDetector::from_probe(probe, config);
        assert!(!detector.is_lcu_available());
        assert_eq!(detector.current_phase(), AgentPhase::Unknown);
    }

    #[test]
    fn phase_detector_with_lcu_in_progress() {
        let probe = LcuProbeResult {
            available: true,
            lockfile_path: "/fake".to_string(),
            lockfile: None,
            phase: Some(GamePhase::InProgress),
            error: None,
        };
        let config = PhaseDetectorConfig::default();
        let detector = PhaseDetector::from_probe(probe, config);
        assert!(detector.is_lcu_available());
        assert_eq!(detector.current_phase(), AgentPhase::ShopPhase);
    }

    #[test]
    fn debounce_requires_consecutive_confirmations() {
        let probe = LcuProbeResult {
            available: false,
            lockfile_path: "/fake".to_string(),
            lockfile: None,
            phase: None,
            error: None,
        };
        let config = PhaseDetectorConfig { debounce_ms: 0, ..Default::default() };
        let mut detector = PhaseDetector::from_probe(probe, config);
        assert_eq!(detector.current_phase(), AgentPhase::Unknown);

        // First call — sets pending, doesn't change yet
        let changed = detector.apply_phase_change(AgentPhase::ShopPhase);
        assert!(!changed);
        assert_eq!(detector.current_phase(), AgentPhase::Unknown);

        // Second call — confirms, changes
        let changed = detector.apply_phase_change(AgentPhase::ShopPhase);
        assert!(changed);
        assert_eq!(detector.current_phase(), AgentPhase::ShopPhase);
    }
}
