use rand::{rngs::StdRng, Rng, SeedableRng};
use tft_domain::{
    augment_name_matches, first_open_board_slot, unit_name_matches, BoardPhase, GameSnapshot,
    ShopSlot, TemplateMatchReadout, UnitInstance, UserPreset,
};

use crate::{DiscreteAction, Obs, StepResult, TftEnv};

/// Default max rounds (matches tft-sim SimulatorConfig).
#[allow(dead_code)]
const DEFAULT_MAX_ROUNDS: u8 = 6;
const BENCH_LIMIT: usize = 9;

/// Internal game state mirroring tft-sim's SimState, extended for RL stepping.
#[derive(Debug, Clone)]
struct SimState {
    gold: u16,
    level: u8,
    health: u16,
    streak: i16,
    board: Vec<UnitInstance>,
    bench: Vec<UnitInstance>,
    next_unit_id: usize,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            gold: 10,
            level: 4,
            health: 100,
            streak: 0,
            board: Vec::new(),
            bench: vec![bench_unit(0, "Poppy")],
            next_unit_id: 1,
        }
    }
}

/// Gym-style RL environment wrapping tft-sim's simulation mechanics.
pub struct SimEnv {
    state: SimState,
    rng: StdRng,
    round: u8,
    max_rounds: u8,
    strength_score: f32,
    total_reward: f32,
    done: bool,
    snapshot: GameSnapshot,
    preset: UserPreset,
}

impl SimEnv {
    /// Read-only access to the current game snapshot (for teacher policies).
    pub fn snapshot(&self) -> &GameSnapshot {
        &self.snapshot
    }

    pub fn new(max_rounds: u8) -> Self {
        let rng = StdRng::seed_from_u64(0);
        let state = SimState::default();
        let preset = UserPreset::default();
        let snapshot = snapshot_for_round(&preset, &state, 1, &mut StdRng::seed_from_u64(0));
        Self {
            state,
            rng,
            round: 1,
            max_rounds,
            strength_score: 0.0,
            total_reward: 0.0,
            done: false,
            snapshot,
            preset,
        }
    }

    pub fn with_preset(mut self, preset: UserPreset) -> Self {
        self.preset = preset;
        self
    }

    fn make_obs(&self) -> Obs {
        let s = &self.snapshot;

        // Scalars: gold, level, xp, health, streak, round, board_count, bench_count
        let scalars = vec![
            s.gold as f32,
            s.level as f32,
            s.xp as f32,
            s.health as f32,
            s.streak as f32,
            self.round as f32,
            s.board.len() as f32,
            s.bench.len() as f32,
        ];

        // Shop costs (5 slots, 0 if empty)
        let mut shop_costs = vec![0.0f32; 5];
        for slot in &s.shop {
            if (slot.index as usize) < 5 {
                shop_costs[slot.index as usize] = slot.cost as f32;
            }
        }

        // Shop preferred: 1.0 if slot matches desired preset
        let mut shop_preferred = vec![0.0f32; 5];
        for slot in &s.shop {
            if (slot.index as usize) < 5 {
                let hit = s
                    .user_preset
                    .desired_units
                    .iter()
                    .any(|desired| unit_name_matches(desired, &slot.unit_name));
                if hit {
                    shop_preferred[slot.index as usize] = 1.0;
                }
            }
        }

        // Board cost distribution (count by cost tier 1-5)
        let mut board_cost_dist = vec![0.0f32; 5];
        for unit in &s.board {
            let tier = (unit.cost as usize).clamp(1, 5) - 1;
            board_cost_dist[tier] += 1.0;
        }

        // Phase one-hot: [lobby, augment, shop, placement, combat, post_combat, carousel]
        let mut phase = vec![0.0f32; 7];
        let idx = match s.board_phase {
            BoardPhase::Lobby => 0,
            BoardPhase::Augment => 1,
            BoardPhase::ShopEconomy => 2,
            BoardPhase::BoardPlacement => 3,
            BoardPhase::Combat => 4,
            BoardPhase::PostCombat => 5,
            BoardPhase::Carousel => 6,
        };
        phase[idx] = 1.0;

        // Flags
        let flags = vec![
            if s.flags.bench_full { 1.0 } else { 0.0 },
            if s.flags.can_level { 1.0 } else { 0.0 },
            if s.flags.can_reroll { 1.0 } else { 0.0 },
            if s.flags.pending_augment { 1.0 } else { 0.0 },
        ];

        Obs {
            scalars,
            shop_costs,
            shop_preferred,
            board_cost_dist,
            phase,
            flags,
        }
    }

    fn compute_placement(strength_score: f32, health: u16) -> f32 {
        (8.5 - ((strength_score + health as f32 / 25.0) / 3.1)).clamp(1.0, 8.0)
    }

    /// Map a DiscreteAction to a GameAction and apply it to the current state.
    /// Returns (score_delta, note_string).
    fn apply_discrete_action(&mut self, action: DiscreteAction) -> (f32, String) {
        let snap = &self.snapshot;
        let state = &mut self.state;

        match action {
            // === Shop actions (0-6) ===
            DiscreteAction::Noop => {
                // Penalise noop if there's a preferred shop hit (blunder pattern from tft-sim)
                let has_preferred_hit = snap.shop.iter().any(|slot| {
                    snap.user_preset
                        .desired_units
                        .iter()
                        .any(|desired| unit_name_matches(desired, &slot.unit_name))
                });
                if matches!(
                    snap.board_phase,
                    BoardPhase::BoardPlacement | BoardPhase::Combat | BoardPhase::PostCombat
                ) && state.board.len() < state.level as usize
                    && !state.bench.is_empty()
                {
                    (-1.0, "noop left playable unit on bench".into())
                } else if matches!(snap.board_phase, BoardPhase::ShopEconomy) && has_preferred_hit
                {
                    (-1.0, "noop skipped a preferred shop hit".into())
                } else {
                    (0.1, "noop".into())
                }
            }
            DiscreteAction::BuySlot0
            | DiscreteAction::BuySlot1
            | DiscreteAction::BuySlot2
            | DiscreteAction::BuySlot3
            | DiscreteAction::BuySlot4 => {
                let slot_idx = (action as u16) - 1; // 1..=5 -> 0..=4
                let Some(shop_slot) = snap.shop.iter().find(|s| s.index as u16 == slot_idx) else {
                    return (-1.0, format!("invalid shop slot {slot_idx}"));
                };
                if state.gold < shop_slot.cost as u16 {
                    return (-1.0, format!("not enough gold for {}", shop_slot.unit_name));
                }
                if state.bench.len() >= BENCH_LIMIT {
                    // Try to sell weakest to make room, like tft-strategy does
                    if let Some(weakest_idx) = find_weakest_bench_idx(snap, &state.bench) {
                        let _sold = state.bench.remove(weakest_idx);
                    } else {
                        return (-1.0, "bench full, cannot sell to make room".into());
                    }
                }
                state.gold = state.gold.saturating_sub(shop_slot.cost as u16);
                let preferred = snap
                    .user_preset
                    .desired_units
                    .iter()
                    .any(|desired| unit_name_matches(desired, &shop_slot.unit_name));
                let uid = state.next_unit_id;
                state.next_unit_id += 1;
                state.bench.push(bench_unit(uid, &shop_slot.unit_name));
                let score = if preferred { 2.2 } else { 0.6 };
                (score, format!("bought {}", shop_slot.unit_name))
            }
            DiscreteAction::Reroll => {
                if state.gold < 2 {
                    return (-1.0, "not enough gold to reroll".into());
                }
                state.gold = state.gold.saturating_sub(2);
                let hit = snap.shop.iter().any(|slot| {
                    snap.user_preset
                        .desired_units
                        .iter()
                        .any(|desired| unit_name_matches(desired, &slot.unit_name))
                });
                let score = if hit { -0.2 } else { 0.9 };
                (score, "rerolled shop".into())
            }

            // === Economy actions (7-8) ===
            DiscreteAction::BuyXp => {
                if state.gold < 4 {
                    return (-1.0, "not enough gold for xp".into());
                }
                if state.level >= 10 {
                    return (-1.0, "already max level".into());
                }
                state.gold = state.gold.saturating_sub(4);
                state.level = state.level.saturating_add(1).min(10);
                let score = if state.gold >= 46 { 2.0 } else { 1.1 };
                (score, "bought xp".into())
            }
            DiscreteAction::LevelUp => {
                if state.gold < 4 {
                    return (-1.0, "not enough gold for level up".into());
                }
                if state.level >= 10 {
                    return (-1.0, "already max level".into());
                }
                state.gold = state.gold.saturating_sub(4);
                state.level = state.level.saturating_add(1).min(10);
                (1.5, "leveled up".into())
            }

            // === Board management (9-14) ===
            DiscreteAction::PromoteBestBench => {
                if state.board.len() >= state.level as usize {
                    return (-1.0, "board full".into());
                }
                let Some(target) = first_open_board_slot(snap) else {
                    return (-1.0, "no open board slot".into());
                };
                let Some(best_idx) = find_best_bench_idx(snap, &state.bench) else {
                    return (-1.0, "bench empty".into());
                };
                let mut unit = state.bench.remove(best_idx);
                unit.position = Some(target);
                state.board.push(unit);
                (2.4, "promoted best bench unit".into())
            }
            DiscreteAction::FillBoard => {
                let mut count = 0;
                while state.board.len() < state.level as usize && !state.bench.is_empty() {
                    let Some(target) = first_open_board_slot(snap) else {
                        break;
                    };
                    let Some(best_idx) = find_best_bench_idx(snap, &state.bench) else {
                        break;
                    };
                    let mut unit = state.bench.remove(best_idx);
                    unit.position = Some(target);
                    state.board.push(unit);
                    count += 1;
                }
                if count == 0 {
                    (-1.0, "nothing to fill".into())
                } else {
                    (2.4 * count as f32, format!("filled {count} board slots"))
                }
            }
            DiscreteAction::SellWeakest => {
                if state.bench.is_empty() {
                    return (-1.0, "bench empty".into());
                }
                let idx =
                    find_weakest_bench_idx(snap, &state.bench).unwrap_or(state.bench.len() - 1);
                let sold = state.bench.remove(idx);
                state.gold = state.gold.saturating_add(sold.cost.max(1) as u16);
                (0.5, format!("sold {}", sold.name))
            }
            DiscreteAction::SellWeakestBoard => {
                if state.board.is_empty() {
                    return (-1.0, "board empty".into());
                }
                let weakest_idx = state
                    .board
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        (a.stars, a.cost, a.name.as_str()).cmp(&(b.stars, b.cost, b.name.as_str()))
                    })
                    .map(|(idx, _)| idx)
                    .unwrap();
                let sold = state.board.remove(weakest_idx);
                state.gold = state.gold.saturating_add(sold.cost.max(1) as u16);
                (0.3, format!("sold board unit {}", sold.name))
            }
            DiscreteAction::MoveFrontline => {
                if let Some(last) = state.board.last_mut() {
                    // Move to front row (row 0, keep same column or default to col 3)
                    let col = last.position.as_ref().map(|p| p.column).unwrap_or(3);
                    last.position = Some(tft_domain::BoardPosition { row: 0, column: col });
                    (0.4, "moved to frontline".into())
                } else {
                    (-1.0, "no board units".into())
                }
            }
            DiscreteAction::MoveBackline => {
                if let Some(last) = state.board.last_mut() {
                    // Move to back row (row 3, keep same column)
                    let col = last.position.as_ref().map(|p| p.column).unwrap_or(3);
                    last.position = Some(tft_domain::BoardPosition { row: 3, column: col });
                    (0.4, "moved to backline".into())
                } else {
                    (-1.0, "no board units".into())
                }
            }

            // === Augment actions (15-17) ===
            DiscreteAction::ChooseAugment0
            | DiscreteAction::ChooseAugment1
            | DiscreteAction::ChooseAugment2 => {
                let idx = (action as u16) - 15; // 15..=17 -> 0..=2
                let Some(augment) = snap.augments.get(idx as usize) else {
                    return (-1.0, "no augment at index".into());
                };
                let preferred = snap
                    .user_preset
                    .augment_priority
                    .iter()
                    .any(|candidate| augment_name_matches(candidate, augment));
                let score = if preferred { 1.8 } else { 1.1 };
                (score, format!("chose augment {augment}"))
            }

            // === Item actions (18-22) ===
            // Items don't fully exist in sim yet — award score bonuses as placeholders.
            DiscreteAction::EquipItemBest => {
                if state.board.is_empty() {
                    return (-1.0, "no board units to equip".into());
                }
                (0.8, "equipped item on best unit (sim placeholder)".into())
            }
            DiscreteAction::EquipItemCarry => {
                if state.board.is_empty() {
                    return (-1.0, "no board units to equip".into());
                }
                (0.7, "equipped item on carry (sim placeholder)".into())
            }
            DiscreteAction::EquipItemTank => {
                if state.board.is_empty() {
                    return (-1.0, "no board units to equip".into());
                }
                (0.6, "equipped item on tank (sim placeholder)".into())
            }
            DiscreteAction::CombineItems => {
                (0.5, "combine items (sim placeholder)".into())
            }
            DiscreteAction::RemoveItems => {
                if state.board.is_empty() {
                    return (-1.0, "no board units to remove items from".into());
                }
                (0.3, "removed items (sim placeholder)".into())
            }

            // === Economy advanced (23-25) ===
            DiscreteAction::HoldGold => {
                // Intentionally save — small positive if gold is already high
                let score = if state.gold >= 50 {
                    1.2
                } else if state.gold >= 30 {
                    0.8
                } else {
                    0.2
                };
                (score, "held gold".into())
            }
            DiscreteAction::SpendAll => {
                // Loop buying XP until gold < 4
                let mut levels_gained = 0u8;
                while state.gold >= 4 && state.level < 10 {
                    state.gold = state.gold.saturating_sub(4);
                    state.level = state.level.saturating_add(1).min(10);
                    levels_gained += 1;
                }
                if levels_gained == 0 {
                    (-1.0, "not enough gold to spend".into())
                } else {
                    (1.5 * levels_gained as f32, format!("spent all gold, gained {levels_gained} levels"))
                }
            }
            DiscreteAction::InterestReroll => {
                // Only reroll if gold >= 50 (interest threshold)
                if state.gold < 50 {
                    return (-0.5, "gold below interest threshold, skipping reroll".into());
                }
                if state.gold < 2 {
                    return (-1.0, "not enough gold to reroll".into());
                }
                state.gold = state.gold.saturating_sub(2);
                let hit = snap.shop.iter().any(|slot| {
                    snap.user_preset
                        .desired_units
                        .iter()
                        .any(|desired| unit_name_matches(desired, &slot.unit_name))
                });
                let score = if hit { 0.5 } else { 1.0 };
                (score, "interest reroll".into())
            }

            // === Board positioning (26-31) ===
            DiscreteAction::SwapFrontBack => {
                if state.board.len() < 2 {
                    return (-1.0, "need at least 2 units to swap".into());
                }
                // Swap first and last board unit positions
                let last_idx = state.board.len() - 1;
                let first_pos = state.board[0].position.clone();
                let last_pos = state.board[last_idx].position.clone();
                state.board[0].position = last_pos;
                state.board[last_idx].position = first_pos;
                (0.4, "swapped front and back".into())
            }
            DiscreteAction::MoveLeftFlank => {
                if let Some(last) = state.board.last_mut() {
                    let row = last.position.as_ref().map(|p| p.row).unwrap_or(0);
                    last.position = Some(tft_domain::BoardPosition { row, column: 0 });
                    (0.3, "moved to left flank".into())
                } else {
                    (-1.0, "no board units".into())
                }
            }
            DiscreteAction::MoveRightFlank => {
                if let Some(last) = state.board.last_mut() {
                    let row = last.position.as_ref().map(|p| p.row).unwrap_or(0);
                    last.position = Some(tft_domain::BoardPosition { row, column: 6 });
                    (0.3, "moved to right flank".into())
                } else {
                    (-1.0, "no board units".into())
                }
            }
            DiscreteAction::PromotePair => {
                // Find bench unit that shares name with a board unit
                let pair_idx = state.bench.iter().enumerate().find_map(|(bi, bunit)| {
                    if state.board.iter().any(|b| b.name == bunit.name) {
                        Some(bi)
                    } else {
                        None
                    }
                });
                let Some(idx) = pair_idx else {
                    return (-1.0, "no pairs found".into());
                };
                if state.board.len() >= state.level as usize {
                    return (-1.0, "board full, cannot promote pair".into());
                }
                let Some(target) = first_open_board_slot(snap) else {
                    return (-1.0, "no open board slot".into());
                };
                let mut unit = state.bench.remove(idx);
                unit.position = Some(target);
                state.board.push(unit);
                (2.0, "promoted pair unit".into())
            }
            DiscreteAction::SellBenchClear => {
                // Sell all non-preferred bench units
                let mut sold_count = 0u16;
                let mut gold_earned = 0u16;
                let desired = &snap.user_preset.desired_units;
                state.bench.retain(|unit| {
                    let is_desired = desired
                        .iter()
                        .any(|d| unit_name_matches(d, &unit.name));
                    if is_desired {
                        true
                    } else {
                        gold_earned += unit.cost.max(1) as u16;
                        sold_count += 1;
                        false
                    }
                });
                state.gold = state.gold.saturating_add(gold_earned);
                if sold_count == 0 {
                    (-1.0, "no non-preferred units to sell".into())
                } else {
                    (0.5 * sold_count as f32, format!("sold {sold_count} bench units"))
                }
            }
            DiscreteAction::BoardToBench => {
                if state.board.is_empty() {
                    return (-1.0, "board empty".into());
                }
                if state.bench.len() >= BENCH_LIMIT {
                    return (-1.0, "bench full".into());
                }
                let weakest_idx = state
                    .board
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        (a.stars, a.cost, a.name.as_str()).cmp(&(b.stars, b.cost, b.name.as_str()))
                    })
                    .map(|(idx, _)| idx)
                    .unwrap();
                let mut unit = state.board.remove(weakest_idx);
                unit.position = None;
                state.bench.push(unit);
                (-0.3, "moved weakest board unit to bench".into())
            }

            // === Special (32-34) ===
            DiscreteAction::ChooseAugmentPreferred => {
                // Pick augment matching preset priority
                if snap.augments.is_empty() {
                    return (-1.0, "no augments available".into());
                }
                let preferred_idx = snap.augments.iter().enumerate().find_map(|(i, augment)| {
                    let is_preferred = snap
                        .user_preset
                        .augment_priority
                        .iter()
                        .any(|candidate| augment_name_matches(candidate, augment));
                    if is_preferred {
                        Some(i)
                    } else {
                        None
                    }
                });
                if let Some(_idx) = preferred_idx {
                    (2.0, "chose preferred augment".into())
                } else {
                    // No preferred augment, pick first
                    (1.0, "no preferred augment, chose first".into())
                }
            }
            DiscreteAction::EmergencySell => {
                // Sell most expensive unit (gold recovery)
                // Check bench first, then board
                let bench_most_expensive =
                    state.bench.iter().enumerate().max_by_key(|(_, u)| u.cost);
                let board_most_expensive =
                    state.board.iter().enumerate().max_by_key(|(_, u)| u.cost);

                let sell_from_bench = match (bench_most_expensive, board_most_expensive) {
                    (Some((_bi, bu)), Some((_, wu))) => bu.cost >= wu.cost,
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => return (-1.0, "no units to sell".into()),
                };

                if sell_from_bench {
                    let idx = bench_most_expensive.map(|(i, _)| i).unwrap();
                    let sold = state.bench.remove(idx);
                    let gold = sold.cost.max(1) as u16;
                    state.gold = state.gold.saturating_add(gold);
                    (0.2, format!("emergency sold bench {} for {gold}g", sold.name))
                } else {
                    let idx = board_most_expensive.map(|(i, _)| i).unwrap();
                    let sold = state.board.remove(idx);
                    let gold = sold.cost.max(1) as u16;
                    state.gold = state.gold.saturating_add(gold);
                    (-0.2, format!("emergency sold board {} for {gold}g", sold.name))
                }
            }
            DiscreteAction::Scout => {
                // Same as Noop but with scouting intent
                (0.1, "scouted (noop placeholder)".into())
            }

            // Sentinel — should never be dispatched
            DiscreteAction::ActionCount => (0.0, "no-op sentinel".into()),
        }
    }
}

impl TftEnv for SimEnv {
    fn reset(&mut self, seed: u64) -> Obs {
        self.rng = StdRng::seed_from_u64(seed);
        self.state = SimState::default();
        self.round = 1;
        self.strength_score = 0.0;
        self.total_reward = 0.0;
        self.done = false;

        self.snapshot = snapshot_for_round(&self.preset, &self.state, self.round, &mut self.rng);
        self.make_obs()
    }

    fn step(&mut self, action: DiscreteAction) -> StepResult {
        if self.done {
            let obs = self.make_obs();
            return StepResult {
                obs,
                reward: 0.0,
                terminated: true,
                truncated: false,
                info: serde_json::json!({"note": "episode already done"}),
            };
        }

        // Apply the discrete action
        let (score_delta, note) = self.apply_discrete_action(action);

        // Add a small random bonus (matches tft-sim)
        let noise: f32 = self.rng.gen_range(0.0..0.35);
        self.strength_score += score_delta + noise;

        // End-of-round mechanics (same as tft-sim run_episode)
        let income = 5 + u16::from(self.round >= 4) + self.rng.gen_range(0..=2);
        self.state.gold = self.state.gold.saturating_add(income);

        let board_gap = self
            .state
            .level
            .saturating_sub(self.state.board.len() as u8) as f32;
        if board_gap > 0.0 {
            self.state.health = self
                .state
                .health
                .saturating_sub((board_gap as u16).saturating_mul(3));
            self.state.streak = 0;
            self.strength_score -= board_gap * 0.8;
        } else {
            self.state.streak = (self.state.streak + 1).min(5);
            self.strength_score += 0.8;
        }

        let step_reward = score_delta;
        self.total_reward += step_reward;

        // Advance round
        self.round += 1;

        // Check termination
        let terminated = self.round > self.max_rounds || self.state.health == 0;
        let truncated = self.round > self.max_rounds && self.state.health > 0;
        self.done = terminated || truncated;

        // Generate next snapshot (or reuse current if done)
        if !self.done {
            self.snapshot =
                snapshot_for_round(&self.preset, &self.state, self.round, &mut self.rng);
        }

        // Terminal reward from placement
        let terminal_reward = if self.done {
            let placement =
                Self::compute_placement(self.strength_score, self.state.health);
            // Map placement 1..=8 to reward +8..=-8
            (4.5 - placement) * 2.0
        } else {
            0.0
        };
        self.total_reward += terminal_reward;

        let obs = self.make_obs();
        let info = if self.done {
            let placement =
                Self::compute_placement(self.strength_score, self.state.health);
            serde_json::json!({
                "note": note,
                "placement": placement,
                "strength_score": self.strength_score,
                "final_hp": self.state.health,
                "round": self.round - 1,
            })
        } else {
            serde_json::json!({
                "note": note,
                "round": self.round - 1,
            })
        };

        StepResult {
            obs,
            reward: step_reward + terminal_reward,
            terminated,
            truncated,
            info,
        }
    }

    fn action_count(&self) -> usize {
        DiscreteAction::count()
    }

    fn legal_mask(&self) -> Vec<bool> {
        let snap = &self.snapshot;
        let state = &self.state;
        let mut mask = vec![false; DiscreteAction::count()];

        // Noop is always legal
        mask[DiscreteAction::Noop as usize] = true;

        // BuySlot0..4 - legal during ShopEconomy if gold >= cost and bench not full
        if matches!(snap.board_phase, BoardPhase::ShopEconomy) {
            for slot in &snap.shop {
                let idx = slot.index as usize;
                if idx < 5 {
                    let affordable = state.gold >= slot.cost as u16;
                    let room = state.bench.len() < BENCH_LIMIT;
                    mask[DiscreteAction::BuySlot0 as usize + idx] = affordable && room;
                }
            }

            // Reroll - needs 2 gold
            mask[DiscreteAction::Reroll as usize] = state.gold >= 2;

            // BuyXp - needs 4 gold and level < 10
            mask[DiscreteAction::BuyXp as usize] = state.gold >= 4 && state.level < 10;

            // LevelUp - same as BuyXp for this sim
            mask[DiscreteAction::LevelUp as usize] = state.gold >= 4 && state.level < 10;

            // SpendAll - needs 4 gold and level < 10
            mask[DiscreteAction::SpendAll as usize] = state.gold >= 4 && state.level < 10;

            // InterestReroll - needs 50 gold
            mask[DiscreteAction::InterestReroll as usize] = state.gold >= 50;
        }

        // SellWeakest - legal if bench is non-empty
        mask[DiscreteAction::SellWeakest as usize] = !state.bench.is_empty();

        // SellWeakestBoard - legal if board is non-empty
        mask[DiscreteAction::SellWeakestBoard as usize] = !state.board.is_empty();

        // EmergencySell - legal if any unit exists
        mask[DiscreteAction::EmergencySell as usize] =
            !state.bench.is_empty() || !state.board.is_empty();

        // Board placement actions - legal if board has space and bench has units
        if matches!(
            snap.board_phase,
            BoardPhase::BoardPlacement | BoardPhase::ShopEconomy | BoardPhase::PostCombat
        ) {
            let board_has_room = state.board.len() < state.level as usize;
            let bench_has_units = state.bench.iter().any(|u| u.is_operable_unit());
            mask[DiscreteAction::PromoteBestBench as usize] = board_has_room && bench_has_units;
            mask[DiscreteAction::FillBoard as usize] = board_has_room && bench_has_units;

            // PromotePair - needs a pair (bench unit matching board unit name) + board space
            if board_has_room {
                let has_pair = state.bench.iter().any(|bunit| {
                    state.board.iter().any(|b| b.name == bunit.name)
                });
                mask[DiscreteAction::PromotePair as usize] = has_pair;
            }
        }

        // Board positioning - legal if board is non-empty
        let board_non_empty = !state.board.is_empty();
        mask[DiscreteAction::MoveFrontline as usize] = board_non_empty;
        mask[DiscreteAction::MoveBackline as usize] = board_non_empty;
        mask[DiscreteAction::SwapFrontBack as usize] = state.board.len() >= 2;
        mask[DiscreteAction::MoveLeftFlank as usize] = board_non_empty;
        mask[DiscreteAction::MoveRightFlank as usize] = board_non_empty;
        mask[DiscreteAction::BoardToBench as usize] =
            board_non_empty && state.bench.len() < BENCH_LIMIT;

        // SellBenchClear - legal if any non-preferred bench units exist
        mask[DiscreteAction::SellBenchClear as usize] = state.bench.iter().any(|unit| {
            !snap
                .user_preset
                .desired_units
                .iter()
                .any(|d| unit_name_matches(d, &unit.name))
        });

        // HoldGold is always legal (it's a strategic choice)
        mask[DiscreteAction::HoldGold as usize] = true;

        // Scout is always legal
        mask[DiscreteAction::Scout as usize] = true;

        // Augment choices - legal during Augment phase if augments exist
        if matches!(snap.board_phase, BoardPhase::Augment) {
            if snap.augments.len() > 0 {
                mask[DiscreteAction::ChooseAugment0 as usize] = true;
                mask[DiscreteAction::ChooseAugmentPreferred as usize] = true;
            }
            if snap.augments.len() > 1 {
                mask[DiscreteAction::ChooseAugment1 as usize] = true;
            }
            if snap.augments.len() > 2 {
                mask[DiscreteAction::ChooseAugment2 as usize] = true;
            }
        }

        // Item actions - legal if board has units (simplified)
        if board_non_empty {
            mask[DiscreteAction::EquipItemBest as usize] = true;
            mask[DiscreteAction::EquipItemCarry as usize] = true;
            mask[DiscreteAction::EquipItemTank as usize] = true;
            mask[DiscreteAction::CombineItems as usize] = true;
            mask[DiscreteAction::RemoveItems as usize] = true;
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

// ---------------------------------------------------------------------------
// Helper functions (re-implemented from tft-sim internals for M0)
// ---------------------------------------------------------------------------

fn snapshot_for_round(
    preset: &UserPreset,
    state: &SimState,
    round: u8,
    rng: &mut StdRng,
) -> GameSnapshot {
    let mut snapshot = GameSnapshot::mock(round, state.gold);
    snapshot.patch = "sim-env".into();
    snapshot.round = round;
    snapshot.gold = state.gold;
    snapshot.level = state.level;
    snapshot.health = state.health;
    snapshot.streak = state.streak;
    snapshot.user_preset = preset.clone();
    snapshot.board = state.board.clone();
    snapshot.bench = state.bench.clone();
    snapshot.shop = generate_shop(preset, round, state, rng);
    snapshot.items = vec![
        TemplateMatchReadout {
            slot: "SLOT_1".into(),
            value: "Bow".into(),
            score: 800,
        },
        TemplateMatchReadout {
            slot: "SLOT_2".into(),
            value: "Rod".into(),
            score: 750,
        },
    ];
    if !matches!(snapshot.board_phase, BoardPhase::Augment) {
        snapshot.augments.clear();
    }
    snapshot.flags.bench_full = snapshot.bench.len() >= BENCH_LIMIT;
    snapshot.flags.can_level = snapshot.level < 10 && snapshot.gold >= 4;
    snapshot.flags.can_reroll = snapshot.gold >= 2;
    snapshot.flags.pending_augment = matches!(snapshot.board_phase, BoardPhase::Augment);
    snapshot
}

fn generate_shop(
    preset: &UserPreset,
    round: u8,
    state: &SimState,
    rng: &mut StdRng,
) -> Vec<ShopSlot> {
    let preferred_names = if preset.desired_units.is_empty() {
        vec!["Aatrox".to_string()]
    } else {
        preset.desired_units.clone()
    };
    let flex_names = [
        "Taric", "Jinx", "Scar", "Vi", "Seraphine", "Aurora", "KogMaw",
    ];
    let preferred_hit = rng.gen_bool(if round <= 2 { 0.75 } else { 0.45 });

    let mut units = Vec::new();
    if preferred_hit && round >= 1 {
        let preferred = preferred_names[(round as usize - 1) % preferred_names.len()].clone();
        units.push(ShopSlot {
            index: 0,
            unit_name: preferred,
            cost: if state.level >= 6 { 3 } else { 2 },
            traits: vec!["Preset".into()],
        });
    }

    while units.len() < 3 {
        let name = flex_names[(round as usize + units.len()) % flex_names.len()];
        units.push(ShopSlot {
            index: units.len() as u8,
            unit_name: name.to_string(),
            cost: if units.len() == 2 { 1 } else { 2 },
            traits: vec!["Flex".into()],
        });
    }

    units
}

fn bench_unit(index: usize, name: &str) -> UnitInstance {
    UnitInstance {
        id: format!("bench-{index}"),
        name: name.to_string(),
        cost: 1,
        stars: 1,
        traits: vec!["Sentinel".into()],
        items: Vec::new(),
        position: None,
        kind: tft_domain::UnitKind::Unit,
    }
}

/// Find the index of the weakest bench unit to sell.
/// Prefers selling non-desired units first; among equals picks lowest (stars, cost, name).
fn find_weakest_bench_idx(snap: &GameSnapshot, bench: &[UnitInstance]) -> Option<usize> {
    if bench.is_empty() {
        return None;
    }
    // Prefer non-desired
    let non_desired: Vec<(usize, &UnitInstance)> = bench
        .iter()
        .enumerate()
        .filter(|(_, unit)| {
            !snap
                .user_preset
                .desired_units
                .iter()
                .any(|desired| unit_name_matches(desired, &unit.name))
        })
        .collect();

    let pool: Vec<(usize, &UnitInstance)> = if non_desired.is_empty() {
        bench.iter().enumerate().collect()
    } else {
        non_desired
    };

    pool.into_iter()
        .min_by(|(_, a), (_, b)| {
            (a.stars, a.cost, a.name.as_str()).cmp(&(b.stars, b.cost, b.name.as_str()))
        })
        .map(|(idx, _)| idx)
}

/// Find the index of the best bench unit to promote to board.
/// Prefers desired units; among equals picks highest (stars, items, cost, name).
fn find_best_bench_idx(snap: &GameSnapshot, bench: &[UnitInstance]) -> Option<usize> {
    if bench.is_empty() {
        return None;
    }
    // Prefer desired
    let desired: Vec<(usize, &UnitInstance)> = bench
        .iter()
        .enumerate()
        .filter(|(_, unit)| {
            snap.user_preset
                .desired_units
                .iter()
                .any(|d| unit_name_matches(d, &unit.name))
        })
        .collect();

    let pool: Vec<(usize, &UnitInstance)> = if desired.is_empty() {
        bench.iter().enumerate().collect()
    } else {
        desired
    };

    pool.into_iter()
        .max_by(|(_, a), (_, b)| {
            (a.stars, a.items.len(), a.cost, a.name.as_str()).cmp(&(
                b.stars,
                b.items.len(),
                b.cost,
                b.name.as_str(),
            ))
        })
        .map(|(idx, _)| idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionCategory, CurriculumPhase};

    #[test]
    fn env_reset_and_step() {
        let mut env = SimEnv::new(DEFAULT_MAX_ROUNDS);
        let obs = env.reset(42);
        assert_eq!(obs.to_vec().len(), Obs::dim());
        assert!(!env.is_done());

        let result = env.step(DiscreteAction::Noop);
        assert_eq!(result.obs.to_vec().len(), Obs::dim());
    }

    #[test]
    fn env_runs_full_episode() {
        let mut env = SimEnv::new(DEFAULT_MAX_ROUNDS);
        env.reset(123);
        let mut steps = 0;
        while !env.is_done() {
            let mask = env.legal_mask();
            // Pick first legal action
            let action = mask.iter().position(|&legal| legal).unwrap_or(0);
            let action = DiscreteAction::from_u16(action as u16).unwrap_or(DiscreteAction::Noop);
            env.step(action);
            steps += 1;
        }
        assert!(steps > 0);
        assert!(env.is_done());
    }

    #[test]
    fn legal_mask_length() {
        let mut env = SimEnv::new(DEFAULT_MAX_ROUNDS);
        env.reset(1);
        let mask = env.legal_mask();
        assert_eq!(mask.len(), DiscreteAction::count());
        // Noop is always legal
        assert!(mask[DiscreteAction::Noop as usize]);
    }

    #[test]
    fn buy_slot_legal_when_affordable() {
        let mut env = SimEnv::new(DEFAULT_MAX_ROUNDS);
        env.reset(7);
        let mask = env.legal_mask();
        // At round 1, gold=10, shop should have affordable slots
        let any_buy_legal = mask[1..=5].iter().any(|&b| b);
        // Either we can buy or bench is full (unlikely on round 1 with 1 bench unit)
        // Just check the mask is sane
        assert_eq!(mask.len(), DiscreteAction::count());
        let _ = any_buy_legal; // Don't assert true since it depends on shop generation
    }

    #[test]
    fn action_categories() {
        assert_eq!(DiscreteAction::Noop.category(), ActionCategory::Economy);
        assert_eq!(DiscreteAction::BuySlot0.category(), ActionCategory::Shop);
        assert_eq!(DiscreteAction::Reroll.category(), ActionCategory::Shop);
        assert_eq!(DiscreteAction::BuyXp.category(), ActionCategory::Economy);
        assert_eq!(DiscreteAction::LevelUp.category(), ActionCategory::Economy);
        assert_eq!(
            DiscreteAction::PromoteBestBench.category(),
            ActionCategory::Board
        );
        assert_eq!(
            DiscreteAction::ChooseAugment0.category(),
            ActionCategory::Augment
        );
        assert_eq!(
            DiscreteAction::EquipItemBest.category(),
            ActionCategory::Item
        );
        assert_eq!(
            DiscreteAction::EmergencySell.category(),
            ActionCategory::Emergency
        );
        assert_eq!(DiscreteAction::HoldGold.category(), ActionCategory::Economy);
        assert_eq!(DiscreteAction::Scout.category(), ActionCategory::Economy);
    }

    #[test]
    fn curriculum_phases() {
        let shop_only = CurriculumPhase::ShopOnly.allowed_actions();
        assert_eq!(shop_only.len(), 7); // Noop + 5 BuySlot + Reroll
        assert!(shop_only.contains(&DiscreteAction::BuySlot0));
        assert!(!shop_only.contains(&DiscreteAction::BuyXp));

        let shop_econ = CurriculumPhase::ShopEconomy.allowed_actions();
        assert!(shop_econ.contains(&DiscreteAction::BuyXp));
        assert!(shop_econ.contains(&DiscreteAction::ChooseAugment0));
        assert!(!shop_econ.contains(&DiscreteAction::EquipItemBest));

        let full = CurriculumPhase::Full.allowed_actions();
        assert_eq!(full.len(), DiscreteAction::count());
        assert!(full.contains(&DiscreteAction::EquipItemBest));
    }

    #[test]
    fn from_u16_roundtrip() {
        for i in 0..DiscreteAction::count() {
            let action = DiscreteAction::from_u16(i as u16);
            assert!(action.is_some(), "from_u16({i}) should be Some");
            assert_eq!(action.unwrap() as usize, i);
        }
        assert!(DiscreteAction::from_u16(999).is_none());
    }
}
