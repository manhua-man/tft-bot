//! Redline emergency stop system.
//! Monitors game state for conditions that require immediate termination.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedlineConfig {
    pub min_health: u16,
    pub max_consecutive_blunders: u32,
    pub max_steps_without_progress: usize,
}

impl Default for RedlineConfig {
    fn default() -> Self {
        Self {
            min_health: 1,
            max_consecutive_blunders: 5,
            max_steps_without_progress: 20,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RedlineMonitor {
    config: RedlineConfig,
    consecutive_blunders: u32,
    steps_without_progress: usize,
    last_strength: f32,
    /// Whether we've received the first strength observation.
    /// Prevents false stall detection when strength starts at 0.0.
    has_initial_strength: bool,
    triggered: bool,
    trigger_reason: Option<String>,
}

impl RedlineMonitor {
    pub fn new(config: RedlineConfig) -> Self {
        Self {
            config,
            consecutive_blunders: 0,
            steps_without_progress: 0,
            last_strength: 0.0,
            has_initial_strength: false,
            triggered: false,
            trigger_reason: None,
        }
    }

    /// Check if a redline condition is met. Returns Some(reason) if triggered.
    pub fn check(&mut self, health: u16, reward: f32, strength: f32) -> Option<String> {
        if self.triggered {
            return self.trigger_reason.clone();
        }

        // Health redline
        if health < self.config.min_health {
            self.triggered = true;
            self.trigger_reason = Some(format!(
                "health {} below minimum {}",
                health, self.config.min_health
            ));
            return self.trigger_reason.clone();
        }

        // Consecutive blunder redline
        if reward < -0.5 {
            self.consecutive_blunders += 1;
            if self.consecutive_blunders >= self.config.max_consecutive_blunders {
                self.triggered = true;
                self.trigger_reason = Some(format!(
                    "{} consecutive blunders (threshold {})",
                    self.consecutive_blunders, self.config.max_consecutive_blunders
                ));
                return self.trigger_reason.clone();
            }
        } else {
            self.consecutive_blunders = 0;
        }

        // Progress stall redline
        if !self.has_initial_strength {
            self.has_initial_strength = true;
            self.last_strength = strength;
        } else if (strength - self.last_strength).abs() < 0.01 {
            self.steps_without_progress += 1;
            if self.steps_without_progress >= self.config.max_steps_without_progress {
                self.triggered = true;
                self.trigger_reason = Some(format!(
                    "no progress for {} steps",
                    self.steps_without_progress
                ));
                return self.trigger_reason.clone();
            }
        } else {
            self.steps_without_progress = 0;
            self.last_strength = strength;
        }

        None
    }

    pub fn is_triggered(&self) -> bool {
        self.triggered
    }

    pub fn reason(&self) -> Option<&str> {
        self.trigger_reason.as_deref()
    }

    /// Phase-aware check. Combat noops don't count as stall;
    /// unverified buys in shop count as blunders.
    ///
    /// `phase`: current AgentPhase string (e.g. "ShopPhase", "Combat", "Augment")
    /// `effect_verified`: Some(true/false) for buy actions, None for non-buy
    pub fn check_with_phase(
        &mut self,
        health: u16,
        reward: f32,
        strength: f32,
        phase: &str,
        effect_verified: Option<bool>,
    ) -> Option<String> {
        if self.triggered {
            return self.trigger_reason.clone();
        }

        // Health redline (always applies)
        if health < self.config.min_health {
            self.triggered = true;
            self.trigger_reason = Some(format!(
                "health {} below minimum {}",
                health, self.config.min_health
            ));
            return self.trigger_reason.clone();
        }

        // Consecutive blunder redline
        // In shop phase: unverified buy = blunder; verified buy resets
        // In other phases: only significant negative rewards count
        let is_blunder = if phase == "ShopPhase" {
            // Unverified buy is a blunder
            effect_verified == Some(false) || reward < -0.5
        } else {
            // In combat/augment, only hard failures count
            reward < -0.8
        };

        if is_blunder {
            self.consecutive_blunders += 1;
            if self.consecutive_blunders >= self.config.max_consecutive_blunders {
                self.triggered = true;
                self.trigger_reason = Some(format!(
                    "{} consecutive blunders in {} (threshold {})",
                    self.consecutive_blunders, phase, self.config.max_consecutive_blunders
                ));
                return self.trigger_reason.clone();
            }
        } else {
            self.consecutive_blunders = 0;
        }

        // Progress stall — only in ShopPhase
        // Combat noops are expected (fighting), so don't count them as stall
        if phase == "ShopPhase" {
            if !self.has_initial_strength {
                // First observation — just record, don't count as stall
                self.has_initial_strength = true;
                self.last_strength = strength;
            } else if (strength - self.last_strength).abs() < 0.01 {
                self.steps_without_progress += 1;
                if self.steps_without_progress >= self.config.max_steps_without_progress {
                    self.triggered = true;
                    self.trigger_reason = Some(format!(
                        "no progress for {} steps in ShopPhase",
                        self.steps_without_progress
                    ));
                    return self.trigger_reason.clone();
                }
            } else {
                self.steps_without_progress = 0;
                self.last_strength = strength;
            }
        }
        // In non-shop phases, reset the stall counter (fighting is expected)
        // but don't reset last_strength so we resume tracking when shop returns

        None
    }

    pub fn reset(&mut self) {
        self.consecutive_blunders = 0;
        self.steps_without_progress = 0;
        self.last_strength = 0.0;
        self.has_initial_strength = false;
        self.triggered = false;
        self.trigger_reason = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combat_noop_does_not_stall() {
        let config = RedlineConfig {
            max_steps_without_progress: 3,
            max_consecutive_blunders: 5,
            ..Default::default()
        };
        let mut redline = RedlineMonitor::new(config);

        // In combat, repeated noops (strength doesn't change) should NOT trigger stall
        for _ in 0..10 {
            assert!(redline.check_with_phase(u16::MAX, 0.1, 0.0, "Combat", None).is_none());
        }
    }

    #[test]
    fn shop_no_progress_triggers_stall() {
        let config = RedlineConfig {
            max_steps_without_progress: 3,
            max_consecutive_blunders: 5,
            ..Default::default()
        };
        let mut redline = RedlineMonitor::new(config);

        // In shop, repeated no-progress should trigger stall
        assert!(redline.check_with_phase(u16::MAX, 0.1, 0.0, "ShopPhase", None).is_none());
        assert!(redline.check_with_phase(u16::MAX, 0.1, 0.0, "ShopPhase", None).is_none());
        assert!(redline.check_with_phase(u16::MAX, 0.1, 0.0, "ShopPhase", None).is_none());
        assert!(redline.check_with_phase(u16::MAX, 0.1, 0.0, "ShopPhase", None).is_some());
    }

    #[test]
    fn unverified_buy_is_blunder() {
        let config = RedlineConfig {
            max_consecutive_blunders: 2,
            max_steps_without_progress: 100,
            ..Default::default()
        };
        let mut redline = RedlineMonitor::new(config);

        assert!(redline.check_with_phase(u16::MAX, 0.6, 0.6, "ShopPhase", Some(false)).is_none());
        assert!(redline.check_with_phase(u16::MAX, 0.6, 1.2, "ShopPhase", Some(false)).is_some());
    }

    #[test]
    fn verified_buy_resets_blunder() {
        let config = RedlineConfig {
            max_consecutive_blunders: 2,
            max_steps_without_progress: 100,
            ..Default::default()
        };
        let mut redline = RedlineMonitor::new(config);

        // One unverified buy
        assert!(redline.check_with_phase(u16::MAX, 0.6, 0.6, "ShopPhase", Some(false)).is_none());
        // Verified buy resets counter
        assert!(redline.check_with_phase(u16::MAX, 2.2, 2.8, "ShopPhase", Some(true)).is_none());
        // Another unverified buy — counter is at 1, not 2
        assert!(redline.check_with_phase(u16::MAX, 0.6, 3.4, "ShopPhase", Some(false)).is_none());
    }
}
