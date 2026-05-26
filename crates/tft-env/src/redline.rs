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
        if (strength - self.last_strength).abs() < 0.01 {
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

    pub fn reset(&mut self) {
        self.consecutive_blunders = 0;
        self.steps_without_progress = 0;
        self.last_strength = 0.0;
        self.triggered = false;
        self.trigger_reason = None;
    }
}
