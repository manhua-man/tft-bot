use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json;
use tft_domain::{unit_name_matches, UserPreset};
use tft_executor::correction::OcrCorrectionDict;
use tft_executor::input::InputDispatcher;
use tft_executor::ocr::OcrEngine;
use tft_executor::shop::ShopReader;
use tft_executor::window::{GameWindow, WindowDiscovery};
use tft_executor::ShopSlotReadout;

use tft_executor::backend::ExecutorBackend;
use tft_executor::verify::verify_buy_effect;

use crate::{CurriculumPhase, DiscreteAction, Obs, StepResult, TftEnv};

/// Type alias for RealEnv using trait objects (from ExecutorBackend).
pub type RealEnvBox =
    RealEnv<Box<dyn WindowDiscovery>, Box<dyn OcrEngine>, Box<dyn InputDispatcher>>;

/// Real-machine environment wrapping tft-executor.
///
/// Supports actions based on the current curriculum phase:
/// - ShopOnly: Noop, BuySlot0-4, Reroll
/// - ShopEconomy: + BuyXp, SellWeakest, HoldGold
/// - Full: all 35 actions
///
/// Actions outside the current curriculum phase receive -1.0 penalty.
/// The episode never terminates on its own (the real game continues).
/// Use `--max-steps` or an external timeout to end episodes.
pub struct RealEnv<W: WindowDiscovery, O: OcrEngine, I: InputDispatcher> {
    window_discovery: W,
    shop_reader: ShopReader<O>,
    input: I,
    window: Option<GameWindow>,
    preset: UserPreset,
    step_count: usize,
    max_steps: usize,
    total_reward: f32,
    done: bool,
    trajectory_path: Option<PathBuf>,
    last_shop: Vec<ShopSlotReadout>,
    last_gold: Option<u16>,
    /// Current curriculum phase — controls which actions are legal.
    curriculum_phase: CurriculumPhase,
}

impl<W: WindowDiscovery, O: OcrEngine, I: InputDispatcher> RealEnv<W, O, I> {
    pub fn new(
        window_discovery: W,
        ocr: O,
        input: I,
        corrections: OcrCorrectionDict,
        preset: UserPreset,
        max_steps: usize,
        trajectory_path: Option<PathBuf>,
    ) -> Self {
        Self {
            window_discovery,
            shop_reader: ShopReader::new(ocr, corrections),
            input,
            window: None,
            preset,
            step_count: 0,
            max_steps,
            total_reward: 0.0,
            done: false,
            trajectory_path,
            last_shop: Vec::new(),
            last_gold: None,
            curriculum_phase: CurriculumPhase::ShopOnly,
        }
    }

    /// Create a RealEnv from an ExecutorBackend (convenience for trait-object mode).
    pub fn from_backend(
        backend: ExecutorBackend,
        preset: UserPreset,
        max_steps: usize,
        trajectory_path: Option<PathBuf>,
    ) -> RealEnvBox {
        RealEnv::new(
            backend.discovery,
            backend.ocr,
            backend.input,
            backend.corrections,
            preset,
            max_steps,
            trajectory_path,
        )
    }

    /// Get the last observed shop slot readouts (for rule policy / verification).
    pub fn last_shop_readouts(&self) -> &[ShopSlotReadout] {
        &self.last_shop
    }

    /// Get the last observed gold (for rule policy / verification).
    pub fn last_gold_value(&self) -> Option<u16> {
        self.last_gold
    }

    /// Get the window reference (for preflight / verification).
    pub fn window(&self) -> Option<&GameWindow> {
        self.window.as_ref()
    }

    /// Get a reference to the shop reader (for verify_buy_effect).
    pub fn shop_reader(&self) -> &ShopReader<O> {
        &self.shop_reader
    }

    /// Read round/stage text from the game window (e.g. "2-1", "3-2", "4-2").
    ///
    /// Used for augment round detection. Returns empty string if no window or OCR fails.
    pub fn read_round_text(&self) -> String {
        match self.window {
            Some(ref w) => self.shop_reader.read_round_text(w),
            None => String::new(),
        }
    }

    /// Set the curriculum phase (controls legal_mask).
    pub fn set_curriculum_phase(&mut self, phase: CurriculumPhase) {
        self.curriculum_phase = phase;
    }

    /// Get the current curriculum phase.
    pub fn curriculum_phase(&self) -> CurriculumPhase {
        self.curriculum_phase
    }

    fn make_obs_from_readout(&self, shop: &[ShopSlotReadout], gold: Option<u16>) -> Obs {
        let mut scalars = vec![0.0f32; 8];
        scalars[0] = gold.unwrap_or(0) as f32;
        // level/xp/health/streak/round are unknown in real mode
        // board_count and bench_count are unknown

        let mut shop_costs = vec![0.0f32; 5];
        let mut shop_preferred = vec![0.0f32; 5];
        for slot in shop {
            let idx = slot.index as usize;
            if idx < 5 {
                // We don't know the cost from OCR alone; set to 1.0 if non-empty
                if !slot.corrected_text.is_empty() {
                    shop_costs[idx] = 1.0; // placeholder
                }
                // Check if matches preset
                let hit = self
                    .preset
                    .desired_units
                    .iter()
                    .any(|desired| unit_name_matches(desired, &slot.corrected_text));
                if hit {
                    shop_preferred[idx] = 1.0;
                }
            }
        }

        // Phase one-hot encoding
        let mut phase = vec![0.0f32; 7];
        phase[2] = 1.0; // ShopEconomy (default for real mode)

        // Flags: bench_full=0, can_level=1, can_reroll=1, pending_augment=0
        let flags = vec![0.0, 1.0, 1.0, 0.0];

        Obs {
            scalars,
            shop_costs,
            shop_preferred,
            board_cost_dist: vec![0.0; 5],
            phase,
            flags,
        }
    }

    fn record_trajectory(
        &self,
        action: DiscreteAction,
        reward: f32,
        shop: &[ShopSlotReadout],
        phase: &str,
        verified: Option<bool>,
    ) {
        let Some(ref path) = self.trajectory_path else {
            return;
        };
        let record = serde_json::json!({
            "step": self.step_count,
            "action": action as u16,
            "reward": reward,
            "gold": self.last_gold,
            "shop": shop.iter().map(|s| &s.corrected_text).collect::<Vec<_>>(),
            "phase": phase,
            "verified": verified,
            "timestamp": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write;
            let _ = writeln!(
                file,
                "{}",
                serde_json::to_string(&record).unwrap_or_default()
            );
        }
    }
}

impl<W: WindowDiscovery, O: OcrEngine, I: InputDispatcher> TftEnv for RealEnv<W, O, I> {
    fn reset(&mut self, _seed: u64) -> Obs {
        // Find game window
        self.window = self.window_discovery.find_game_window().ok();
        self.step_count = 0;
        self.total_reward = 0.0;
        self.done = false;

        // Initial read
        if let Some(ref window) = self.window {
            self.last_shop = self.shop_reader.read_shop(window).unwrap_or_default();
            self.last_gold = self.shop_reader.read_gold(window).ok();
        }

        self.make_obs_from_readout(&self.last_shop, self.last_gold)
    }

    fn step(&mut self, action: DiscreteAction) -> StepResult {
        if self.done {
            return StepResult {
                obs: self.make_obs_from_readout(&self.last_shop, self.last_gold),
                reward: 0.0,
                terminated: true,
                truncated: false,
                info: serde_json::json!({"note": "episode already done"}),
            };
        }

        // Check if action is legal in current curriculum phase
        let allowed = self.curriculum_phase.allowed_actions();
        let is_legal = allowed.contains(&action);

        if !is_legal {
            self.step_count += 1;
            self.total_reward += -1.0;
            let obs = self.make_obs_from_readout(&self.last_shop, self.last_gold);
            return StepResult {
                obs,
                reward: -1.0,
                terminated: self.step_count >= self.max_steps,
                truncated: self.step_count >= self.max_steps,
                info: serde_json::json!({
                    "note": format!("illegal action {:?} in {:?} phase", action, self.curriculum_phase),
                    "step": self.step_count,
                }),
            };
        }

        self.step_count += 1;
        let reward;
        let note;
        let mut effect_verified: Option<bool> = None;

        // Check window still available
        if self.window.is_none() {
            self.window = self.window_discovery.find_game_window().ok();
        }
        let Some(ref window) = self.window else {
            self.done = true;
            return StepResult {
                obs: self.make_obs_from_readout(&[], None),
                reward: -1.0,
                terminated: true,
                truncated: false,
                info: serde_json::json!({"note": "game window not found"}),
            };
        };

        // Read current state
        let shop = self.shop_reader.read_shop(window).unwrap_or_default();
        let gold = self.shop_reader.read_gold(window).ok();

        match action {
            DiscreteAction::Noop => {
                reward = 0.1;
                note = "noop";
            }
            DiscreteAction::BuySlot0
            | DiscreteAction::BuySlot1
            | DiscreteAction::BuySlot2
            | DiscreteAction::BuySlot3
            | DiscreteAction::BuySlot4 => {
                let slot = (action as u16) - 1; // BuySlot0=1 -> slot 0, etc.
                let gold_before = gold;
                let shop_before = shop.clone();
                match self.input.buy_slot(window, slot as u8) {
                    Ok(()) => {
                        // Verify buy effect via gold/shop change detection
                        match verify_buy_effect(
                            &self.shop_reader,
                            window,
                            gold_before,
                            &shop_before,
                            slot as u8,
                        ) {
                            Ok(vr) if vr.effect_verified => {
                                let preferred = shop_before.get(slot as usize).map_or(false, |s| {
                                    self.preset
                                        .desired_units
                                        .iter()
                                        .any(|d| unit_name_matches(d, &s.corrected_text))
                                });
                                reward = if preferred { 2.2 } else { 0.6 };
                                note = if preferred {
                                    "bought preferred (verified)"
                                } else {
                                    "bought unit (verified)"
                                };
                                effect_verified = Some(true);
                            }
                            Ok(_) => {
                                // Buy input sent but no gold/shop change detected
                                reward = -0.5;
                                note = "buy unverified (no effect detected)";
                                effect_verified = Some(false);
                            }
                            Err(e) => {
                                reward = -0.8;
                                note = "buy verify error";
                                effect_verified = Some(false);
                                let _ = e; // log if needed
                            }
                        }
                    }
                    Err(_e) => {
                        reward = -1.0;
                        note = "buy failed (input error)";
                        effect_verified = Some(false);
                    }
                }
            }
            DiscreteAction::Reroll => match self.input.reroll(window) {
                Ok(()) => {
                    reward = 0.9;
                    note = "reroll";
                }
                Err(_e) => {
                    reward = -1.0;
                    note = "reroll failed";
                }
            },
            DiscreteAction::BuyXp | DiscreteAction::LevelUp => {
                // Both map to F key in TFT
                match self.input.buy_xp(window) {
                    Ok(()) => {
                        reward = 0.5;
                        note = "buy_xp";
                    }
                    Err(_e) => {
                        reward = -1.0;
                        note = "buy_xp failed";
                    }
                }
            }
            DiscreteAction::SellWeakest | DiscreteAction::SellWeakestBoard => {
                // Sell unit under cursor (E key)
                match self.input.sell_hovered(window) {
                    Ok(()) => {
                        reward = 0.3;
                        note = "sell";
                    }
                    Err(_e) => {
                        reward = -1.0;
                        note = "sell failed";
                    }
                }
            }
            DiscreteAction::HoldGold => {
                reward = 0.1;
                note = "hold_gold";
            }
            DiscreteAction::ChooseAugment0
            | DiscreteAction::ChooseAugment1
            | DiscreteAction::ChooseAugment2 => {
                let slot = (action as u16) - 15; // ChooseAugment0=15 -> slot 0
                match self.input.click_augment(window, slot as u8) {
                    Ok(()) => {
                        reward = 1.5;
                        note = "augment chosen";
                    }
                    Err(_e) => {
                        reward = -1.0;
                        note = "augment click failed";
                    }
                }
            }
            _ => {
                // Other actions not yet implemented on real machine
                reward = -1.0;
                note = "unimplemented action";
            }
        }

        self.total_reward += reward;
        // Determine phase string for trajectory
        let phase_str = if self.curriculum_phase.allowed_actions().is_empty() {
            "unknown"
        } else {
            "shop" // simplified; phase detector provides real phase in run-afk
        };
        self.record_trajectory(action, reward, &shop, phase_str, effect_verified);
        self.last_shop = shop.clone();
        self.last_gold = gold;

        // Check termination (only via max steps)
        let terminated = self.step_count >= self.max_steps;
        let truncated = terminated;
        self.done = terminated;

        let obs = self.make_obs_from_readout(&shop, gold);
        let info = serde_json::json!({
            "note": note,
            "step": self.step_count,
            "gold": gold,
            "total_reward": self.total_reward,
            "curriculum": format!("{:?}", self.curriculum_phase),
            "effect_verified": effect_verified,
        });

        StepResult {
            obs,
            reward,
            terminated,
            truncated,
            info,
        }
    }

    fn action_count(&self) -> usize {
        DiscreteAction::count()
    }

    fn legal_mask(&self) -> Vec<bool> {
        let allowed = self.curriculum_phase.allowed_actions();
        let mut mask = vec![false; DiscreteAction::count()];
        for action in allowed {
            // Only include actions that are actually implemented on real machine.
            // Unimplemented actions (augments, items, positioning, etc.)
            // silently return -1.0 which creates confusing training signal.
            if is_real_machine_action(action) {
                mask[action as usize] = true;
            }
        }
        mask
    }

    fn obs_dim(&self) -> usize {
        Obs::dim()
    }

    fn is_done(&self) -> bool {
        self.done
    }
}

/// Check if a DiscreteAction is actually implemented on the real machine.
///
/// Unimplemented actions (augments, items, board positioning, etc.)
/// return -1.0 in step() and should not appear in legal_mask.
fn is_real_machine_action(action: DiscreteAction) -> bool {
    matches!(
        action,
        DiscreteAction::Noop
            | DiscreteAction::BuySlot0
            | DiscreteAction::BuySlot1
            | DiscreteAction::BuySlot2
            | DiscreteAction::BuySlot3
            | DiscreteAction::BuySlot4
            | DiscreteAction::Reroll
            | DiscreteAction::BuyXp
            | DiscreteAction::LevelUp
            | DiscreteAction::SellWeakest
            | DiscreteAction::SellWeakestBoard
            | DiscreteAction::HoldGold
            | DiscreteAction::ChooseAugment0
            | DiscreteAction::ChooseAugment1
            | DiscreteAction::ChooseAugment2
    )
}
