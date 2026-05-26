use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use tft_domain::{
    augment_name_matches, unit_name_matches, ActionPlan, BoardPhase, GameAction, GameSnapshot,
    ShopSlot, TemplateMatchReadout, UnitInstance, UserPreset,
};
use tft_strategy::StrategyKernel;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatorConfig {
    pub max_rounds: u8,
}

impl Default for SimulatorConfig {
    fn default() -> Self {
        Self { max_rounds: 6 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeRecord {
    pub seed: u64,
    pub kernel_id: String,
    pub placement: f32,
    pub blunders: u32,
    pub snapshots: Vec<GameSnapshot>,
    pub plans: Vec<ActionPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointScenario {
    pub id: String,
    pub snapshot: GameSnapshot,
    pub forbid_noop: bool,
    pub require_board_fill: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointOutcome {
    pub scenario_id: String,
    pub blunder: bool,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct SeededSimulator {
    pub config: SimulatorConfig,
}

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

#[derive(Debug, Clone)]
struct AppliedPlan {
    score_delta: f32,
    blunders: u32,
    gold: u16,
    level: u8,
    board: Vec<UnitInstance>,
    bench: Vec<UnitInstance>,
    note: String,
}

impl SeededSimulator {
    pub fn new(config: SimulatorConfig) -> Self {
        Self { config }
    }

    pub fn run_episode<K: StrategyKernel>(
        &self,
        kernel: &K,
        preset: UserPreset,
        seed: u64,
    ) -> EpisodeRecord {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut state = SimState::default();
        let mut snapshots = Vec::new();
        let mut plans = Vec::new();
        let mut blunders = 0u32;
        let mut strength_score = 0f32;

        for round in 1..=self.config.max_rounds {
            let snapshot = snapshot_for_round(&preset, &state, round, &mut rng);
            let plan = kernel.plan(&snapshot);
            let applied = apply_plan(&snapshot, &plan, &state);

            state.gold = applied
                .gold
                .saturating_add(income_for_round(round, &mut rng));
            state.level = applied.level;
            state.board = applied.board;
            state.bench = applied.bench;
            state.next_unit_id = state
                .next_unit_id
                .max(state.board.len() + state.bench.len() + 1);

            let board_gap = state.level.saturating_sub(state.board.len() as u8) as f32;
            if board_gap > 0.0 {
                state.health = state
                    .health
                    .saturating_sub((board_gap as u16).saturating_mul(3));
                state.streak = 0;
                strength_score -= board_gap * 0.8;
            } else {
                state.streak = (state.streak + 1).min(5);
                strength_score += 0.8;
            }

            strength_score += applied.score_delta + rng.gen_range(0.0..0.35);
            blunders += applied.blunders;
            snapshots.push(snapshot);
            plans.push(plan);
        }

        let placement =
            (8.5 - ((strength_score + state.health as f32 / 25.0) / 3.1)).clamp(1.0, 8.0);

        EpisodeRecord {
            seed,
            kernel_id: kernel.id().to_string(),
            placement,
            blunders,
            snapshots,
            plans,
        }
    }
}

pub fn run_checkpoints<K: StrategyKernel>(
    kernel: &K,
    scenarios: &[CheckpointScenario],
) -> Vec<CheckpointOutcome> {
    scenarios
        .iter()
        .map(|scenario| {
            let state = SimState {
                gold: scenario.snapshot.gold,
                level: scenario.snapshot.level.max(1),
                health: scenario.snapshot.health,
                streak: scenario.snapshot.streak,
                board: scenario.snapshot.board.clone(),
                bench: scenario.snapshot.bench.clone(),
                next_unit_id: scenario.snapshot.board.len() + scenario.snapshot.bench.len() + 1,
            };
            let plan = kernel.plan(&scenario.snapshot);
            let applied = apply_plan(&scenario.snapshot, &plan, &state);
            let is_noop = plan
                .actions
                .iter()
                .all(|action| matches!(action, GameAction::Noop { .. }));
            let board_filled = applied.board.len() >= scenario.snapshot.level as usize;
            let blunder = applied.blunders > 0
                || (scenario.forbid_noop && is_noop)
                || (scenario.require_board_fill && !board_filled);

            let note = if applied.blunders > 0 {
                applied.note
            } else if scenario.forbid_noop && is_noop {
                "checkpoint rejected noop".into()
            } else if scenario.require_board_fill && !board_filled {
                "checkpoint expected a valid board fill".into()
            } else {
                "checkpoint satisfied".into()
            };

            CheckpointOutcome {
                scenario_id: scenario.id.clone(),
                blunder,
                note,
            }
        })
        .collect()
}

fn snapshot_for_round(
    preset: &UserPreset,
    state: &SimState,
    round: u8,
    rng: &mut StdRng,
) -> GameSnapshot {
    let mut snapshot = GameSnapshot::mock(round, state.gold);
    snapshot.patch = "sim-benchmark".into();
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
    snapshot.flags.bench_full = snapshot.bench.len() >= 9;
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
        "Taric",
        "Jinx",
        "Scar",
        "Vi",
        "Seraphine",
        "Aurora",
        "KogMaw",
    ];
    let preferred_hit = rng.gen_bool(if round <= 2 { 0.75 } else { 0.45 });

    let mut units = Vec::new();
    if preferred_hit {
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

fn income_for_round(round: u8, rng: &mut StdRng) -> u16 {
    5 + u16::from(round >= 4) + rng.gen_range(0..=2)
}

fn apply_plan(snapshot: &GameSnapshot, plan: &ActionPlan, state: &SimState) -> AppliedPlan {
    let mut gold = state.gold;
    let mut level = state.level.max(1);
    let mut board = state.board.clone();
    let mut bench = state.bench.clone();
    let mut score_delta = 0.0;
    let mut blunders = 0u32;
    let mut notes = Vec::new();

    for action in &plan.actions {
        match action {
            GameAction::QueueAccept => {
                score_delta += 0.1;
                notes.push("accepted ready check".to_string());
            }
            GameAction::BuyUnit { slot } => {
                let Some(shop_slot) = snapshot
                    .shop
                    .iter()
                    .find(|candidate| candidate.index == *slot)
                else {
                    blunders += 1;
                    notes.push(format!("invalid shop slot {slot}"));
                    continue;
                };
                if gold < shop_slot.cost as u16 || bench.len() >= 9 {
                    blunders += 1;
                    notes.push(format!("buy_unit illegal for {}", shop_slot.unit_name));
                    continue;
                }

                gold = gold.saturating_sub(shop_slot.cost as u16);
                let preferred = snapshot
                    .user_preset
                    .desired_units
                    .iter()
                    .any(|desired| unit_name_matches(desired, &shop_slot.unit_name));
                let unit = bench_unit(board.len() + bench.len() + 1, &shop_slot.unit_name);
                bench.push(unit);
                score_delta += if preferred { 2.2 } else { 0.6 };
                notes.push(format!("bought {}", shop_slot.unit_name));
            }
            GameAction::BuyXp => {
                if gold < 4 || !snapshot.flags.can_level {
                    blunders += 1;
                    notes.push("buy_xp illegal".into());
                    continue;
                }
                gold = gold.saturating_sub(4);
                level = level.saturating_add(1).min(10);
                score_delta += if gold >= 46 { 2.0 } else { 1.1 };
                notes.push("bought xp".into());
            }
            GameAction::Reroll => {
                if gold < 2 || !snapshot.flags.can_reroll {
                    blunders += 1;
                    notes.push("reroll illegal".into());
                    continue;
                }
                gold = gold.saturating_sub(2);
                score_delta += if preferred_shop_hit(snapshot) {
                    -0.2
                } else {
                    0.9
                };
                notes.push("rerolled shop".into());
            }
            GameAction::MoveBoard { unit_id, to } => {
                if board.iter().any(|unit| {
                    unit.position.as_ref().is_some_and(|position| {
                        position.row == to.row && position.column == to.column
                    })
                }) {
                    blunders += 1;
                    notes.push(format!("target slot already occupied for {}", unit_id));
                    continue;
                }

                if let Some(index) = bench.iter().position(|unit| unit.id == *unit_id) {
                    if board.len() >= level as usize {
                        blunders += 1;
                        notes.push(format!("board already full for {}", unit_id));
                        continue;
                    }

                    let mut unit = bench.remove(index);
                    unit.position = Some(to.clone());
                    board.push(unit);
                    score_delta += 2.4;
                    notes.push(format!("promoted {} from bench", unit_id));
                } else if let Some(unit) = board.iter_mut().find(|unit| unit.id == *unit_id) {
                    unit.position = Some(to.clone());
                    score_delta += 0.3;
                    notes.push(format!("repositioned {}", unit_id));
                } else {
                    blunders += 1;
                    notes.push(format!("unknown move target {}", unit_id));
                }
            }
            GameAction::ChooseAugment { index } => {
                let Some(augment) = snapshot.augments.get(*index as usize) else {
                    blunders += 1;
                    notes.push("choose_augment illegal".into());
                    continue;
                };
                let preferred = snapshot
                    .user_preset
                    .augment_priority
                    .iter()
                    .any(|candidate| augment_name_matches(candidate, augment));
                score_delta += if preferred { 1.8 } else { 1.1 };
                notes.push(format!("chose augment {}", augment));
            }
            GameAction::EquipItem { .. } => {
                score_delta += 0.6;
                notes.push("equipped item".into());
            }
            GameAction::Noop { reason } => {
                if matches!(
                    snapshot.board_phase,
                    BoardPhase::BoardPlacement | BoardPhase::Combat | BoardPhase::PostCombat
                ) && board.len() < level as usize
                    && !bench.is_empty()
                {
                    blunders += 1;
                    notes.push("noop left playable unit on bench".into());
                } else if matches!(snapshot.board_phase, BoardPhase::ShopEconomy)
                    && preferred_shop_hit(snapshot)
                {
                    blunders += 1;
                    notes.push("noop skipped a preferred shop hit".into());
                } else {
                    score_delta += 0.1;
                    notes.push(reason.clone());
                }
            }
            _ => {}
        }
    }

    AppliedPlan {
        score_delta,
        blunders,
        gold,
        level,
        board,
        bench,
        note: if notes.is_empty() {
            "no simulator note".into()
        } else {
            notes.join(" / ")
        },
    }
}

fn preferred_shop_hit(snapshot: &GameSnapshot) -> bool {
    snapshot.shop.iter().any(|slot| {
        snapshot
            .user_preset
            .desired_units
            .iter()
            .any(|desired| unit_name_matches(desired, &slot.unit_name))
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use tft_domain::{first_open_board_slot, BoardPosition, ModelMetadata};
    use tft_strategy::{LearnedKernel, PhaseRouter};

    #[test]
    fn simulator_runs_episode() {
        let kernel = PhaseRouter::new(LearnedKernel::new(ModelMetadata {
            id: "m1".into(),
            version: "0.1".into(),
            family: "policy".into(),
            onnx_path: "artifacts/m1.onnx".into(),
            training_dataset: "dataset".into(),
        }));
        let sim = SeededSimulator::new(SimulatorConfig::default());
        let record = sim.run_episode(&kernel, UserPreset::default(), 7);
        assert_eq!(record.seed, 7);
        assert!(!record.plans.is_empty());
    }

    #[test]
    fn move_board_requires_real_unit_id() {
        let mut snapshot = GameSnapshot::mock(2, 12);
        snapshot.board_phase = BoardPhase::BoardPlacement;
        snapshot.level = 4;
        snapshot.bench = vec![bench_unit(0, "Poppy")];
        snapshot.board.clear();
        let state = SimState::default();
        let invalid = apply_plan(
            &snapshot,
            &ActionPlan {
                kernel_id: "rule".into(),
                summary: "invalid".into(),
                confidence: 0.5,
                actions: vec![GameAction::MoveBoard {
                    unit_id: "bench-anchor".into(),
                    to: BoardPosition { row: 0, column: 0 },
                }],
            },
            &state,
        );
        assert!(invalid.blunders > 0);
    }

    #[test]
    fn move_board_promotes_bench_unit() {
        let mut snapshot = GameSnapshot::mock(2, 12);
        snapshot.board_phase = BoardPhase::BoardPlacement;
        snapshot.level = 4;
        snapshot.bench = vec![bench_unit(0, "Poppy")];
        snapshot.board.clear();
        let state = SimState::default();
        let target = first_open_board_slot(&snapshot).expect("open slot");
        let valid = apply_plan(
            &snapshot,
            &ActionPlan {
                kernel_id: "search".into(),
                summary: "valid".into(),
                confidence: 0.8,
                actions: vec![GameAction::MoveBoard {
                    unit_id: "bench-0".into(),
                    to: target,
                }],
            },
            &state,
        );
        assert_eq!(valid.blunders, 0);
        assert_eq!(valid.board.len(), 1);
        assert!(valid.bench.is_empty());
    }

    #[test]
    fn preferred_shop_hit_matches_localized_preset_against_canonical_shop() {
        let mut snapshot = GameSnapshot::mock(2, 12);
        snapshot.user_preset.desired_units = vec!["亚托克斯".into()];
        snapshot.shop = vec![ShopSlot {
            index: 0,
            unit_name: "Aatrox".into(),
            cost: 3,
            traits: vec![],
        }];

        assert!(preferred_shop_hit(&snapshot));
    }

    #[test]
    fn apply_plan_scores_english_augment_priority_against_localized_choice() {
        let mut snapshot = GameSnapshot::mock(3, 12);
        snapshot.board_phase = BoardPhase::Augment;
        snapshot.user_preset.augment_priority = vec!["Cybernetic Uplink".into()];
        snapshot.augments = vec!["组件百宝袋".into(), "源计划上行链路 III".into()];
        let state = SimState::default();

        let applied = apply_plan(
            &snapshot,
            &ActionPlan {
                kernel_id: "search".into(),
                summary: "augment".into(),
                confidence: 0.8,
                actions: vec![GameAction::ChooseAugment { index: 1 }],
            },
            &state,
        );

        assert_eq!(applied.blunders, 0);
        assert!(applied.score_delta > 1.5);
    }
}
