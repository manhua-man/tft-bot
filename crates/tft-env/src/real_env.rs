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

use crate::{DiscreteAction, Obs, StepResult, TftEnv};

/// Real-machine shop-only environment wrapping tft-executor.
///
/// Only exposes shop-related actions: Noop, BuySlot0-4, Reroll.
/// All other actions receive -1.0 penalty (illegal in shop-only mode).
///
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
        }
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

        // Phase: assume shop economy (we're in shop-only mode)
        let mut phase = vec![0.0f32; 7];
        phase[2] = 1.0; // ShopEconomy

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

    fn record_trajectory(&self, action: DiscreteAction, reward: f32, shop: &[ShopSlotReadout]) {
        let Some(ref path) = self.trajectory_path else {
            return;
        };
        let record = serde_json::json!({
            "step": self.step_count,
            "action": action as u16,
            "reward": reward,
            "gold": self.last_gold,
            "shop": shop.iter().map(|s| &s.corrected_text).collect::<Vec<_>>(),
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

        self.step_count += 1;
        let reward;
        let note;

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
                match self.input.buy_slot(window, slot as u8) {
                    Ok(()) => {
                        // Check if preferred
                        let preferred = shop.get(slot as usize).map_or(false, |s| {
                            self.preset
                                .desired_units
                                .iter()
                                .any(|d| unit_name_matches(d, &s.corrected_text))
                        });
                        reward = if preferred { 2.2 } else { 0.6 };
                        note = if preferred {
                            "bought preferred"
                        } else {
                            "bought unit"
                        };
                    }
                    Err(_e) => {
                        reward = -1.0;
                        note = "buy failed";
                        // Don't mark as done - might recover
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
            _ => {
                reward = -1.0;
                note = "illegal action in shop-only mode";
            }
        }

        self.total_reward += reward;
        self.record_trajectory(action, reward, &shop);
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
        let mut mask = vec![false; DiscreteAction::count()];
        // In shop-only mode, only shop actions are legal
        mask[DiscreteAction::Noop as usize] = true;
        mask[DiscreteAction::BuySlot0 as usize] = true;
        mask[DiscreteAction::BuySlot1 as usize] = true;
        mask[DiscreteAction::BuySlot2 as usize] = true;
        mask[DiscreteAction::BuySlot3 as usize] = true;
        mask[DiscreteAction::BuySlot4 as usize] = true;
        mask[DiscreteAction::Reroll as usize] = true;
        mask
    }

    fn obs_dim(&self) -> usize {
        Obs::dim()
    }

    fn is_done(&self) -> bool {
        self.done
    }
}
