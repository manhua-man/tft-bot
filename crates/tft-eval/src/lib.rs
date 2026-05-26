use rand::Rng;
use serde::{Deserialize, Serialize};
use tft_domain::{ActionPlan, GameAction, GameSnapshot};
use tft_env::{DiscreteAction, Obs, StepResult, TftEnv};
use tft_env::sim_env::SimEnv;
use tft_strategy::{PhaseRouter, RuleKernel, StrategyKernel};

// ---------------------------------------------------------------------------
// Policy trait
// ---------------------------------------------------------------------------

/// A policy selects a discrete action given the current observation,
/// the legal-action mask, and (optionally) the raw game snapshot.
pub trait Policy {
    fn select_action(
        &mut self,
        obs: &Obs,
        mask: &[bool],
        snapshot: Option<&GameSnapshot>,
    ) -> DiscreteAction;
}

// ---------------------------------------------------------------------------
// RandomPolicy – uniform over legal actions
// ---------------------------------------------------------------------------

pub struct RandomPolicy {
    rng: rand::rngs::StdRng,
}

impl RandomPolicy {
    pub fn new(seed: u64) -> Self {
        use rand::SeedableRng;
        Self {
            rng: rand::rngs::StdRng::seed_from_u64(seed),
        }
    }
}

impl Policy for RandomPolicy {
    fn select_action(
        &mut self,
        _obs: &Obs,
        mask: &[bool],
        _snapshot: Option<&GameSnapshot>,
    ) -> DiscreteAction {
        let legal: Vec<usize> = mask
            .iter()
            .enumerate()
            .filter(|(_, &ok)| ok)
            .map(|(i, _)| i)
            .collect();
        if legal.is_empty() {
            return DiscreteAction::Noop;
        }
        let idx = self.rng.gen_range(0..legal.len());
        DiscreteAction::from_u16(legal[idx] as u16).unwrap_or(DiscreteAction::Noop)
    }
}

// ---------------------------------------------------------------------------
// RuleTeacherPolicy – wraps a StrategyKernel and maps GameAction → DiscreteAction
// ---------------------------------------------------------------------------

pub struct RuleTeacherPolicy {
    kernel: PhaseRouter<RuleKernel>,
}

impl RuleTeacherPolicy {
    pub fn new() -> Self {
        Self {
            kernel: PhaseRouter::new(RuleKernel::default()),
        }
    }

    /// Map the first GameAction from an ActionPlan into a DiscreteAction.
    pub fn plan_to_discrete(plan: &ActionPlan) -> DiscreteAction {
        match plan.actions.first() {
            Some(action) => game_action_to_discrete(action),
            None => DiscreteAction::Noop,
        }
    }
}

impl Default for RuleTeacherPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl Policy for RuleTeacherPolicy {
    fn select_action(
        &mut self,
        _obs: &Obs,
        mask: &[bool],
        snapshot: Option<&GameSnapshot>,
    ) -> DiscreteAction {
        let Some(snap) = snapshot else {
            return DiscreteAction::Noop;
        };

        let plan = self.kernel.plan(snap);
        let action = Self::plan_to_discrete(&plan);

        // Verify the chosen action is legal; fall back to Noop if not.
        let idx = action as usize;
        if idx < mask.len() && mask[idx] {
            action
        } else {
            DiscreteAction::Noop
        }
    }
}

/// Map a GameAction to the nearest DiscreteAction.
pub fn game_action_to_discrete(action: &GameAction) -> DiscreteAction {
    match action {
        GameAction::Noop { .. } => DiscreteAction::Noop,
        GameAction::BuyUnit { slot } => {
            if *slot < 5 {
                DiscreteAction::from_u16(DiscreteAction::BuySlot0 as u16 + *slot as u16)
                    .unwrap_or(DiscreteAction::Noop)
            } else {
                DiscreteAction::Noop
            }
        }
        GameAction::Reroll => DiscreteAction::Reroll,
        GameAction::BuyXp => DiscreteAction::BuyXp,
        GameAction::SellUnit { .. } => DiscreteAction::SellWeakest,
        GameAction::MoveBoard { .. } => DiscreteAction::PromoteBestBench,
        GameAction::ChooseAugment { index } => {
            if *index < 3 {
                DiscreteAction::from_u16(DiscreteAction::ChooseAugment0 as u16 + *index as u16)
                    .unwrap_or(DiscreteAction::Noop)
            } else {
                DiscreteAction::Noop
            }
        }
        // No discrete-space equivalents for these actions
        GameAction::EquipItem { .. }
        | GameAction::QueueAccept
        | GameAction::MoveBench { .. } => DiscreteAction::Noop,
    }
}

// ---------------------------------------------------------------------------
// Evaluator – run N episodes and collect statistics
// ---------------------------------------------------------------------------

/// Outcome of a single episode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeResult {
    pub seed: u64,
    pub total_return: f32,
    pub placement: f32,
    pub steps: usize,
    pub final_hp: u16,
}

/// Aggregated statistics over a batch of episodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub n_episodes: usize,
    pub return_mean: f32,
    pub return_std: f32,
    pub return_min: f32,
    pub return_max: f32,
    pub placement_mean: f32,
    pub placement_std: f32,
    pub placement_min: f32,
    pub placement_max: f32,
    pub episodes: Vec<EpisodeResult>,
}

impl std::fmt::Display for EvalReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== EvalReport ({} episodes) ===", self.n_episodes)?;
        writeln!(
            f,
            "  Return:    mean={:.2}  std={:.2}  min={:.2}  max={:.2}",
            self.return_mean, self.return_std, self.return_min, self.return_max
        )?;
        writeln!(
            f,
            "  Placement: mean={:.2}  std={:.2}  min={:.2}  max={:.2}",
            self.placement_mean, self.placement_std, self.placement_min, self.placement_max
        )?;
        Ok(())
    }
}

fn compute_stats(values: &[f32]) -> (f32, f32, f32, f32) {
    let n = values.len() as f32;
    if n == 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mean = values.iter().sum::<f32>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n;
    let std = variance.sqrt();
    let min = values.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    (mean, std, min, max)
}

/// Run `policy` over the given `seeds` for up to `max_rounds` each,
/// collecting per-episode returns and placements.
pub fn evaluate_policy(
    policy: &mut dyn Policy,
    seeds: &[u64],
    max_rounds: u8,
) -> EvalReport {
    let mut episodes = Vec::with_capacity(seeds.len());

    for &seed in seeds {
        let mut env = SimEnv::new(max_rounds);
        let mut obs = env.reset(seed);
        let mut total_return: f32 = 0.0;
        let mut steps: usize = 0;

        while !env.is_done() {
            let mask = env.legal_mask();
            let snap = env.snapshot().clone();
            let action = policy.select_action(&obs, &mask, Some(&snap));
            let result: StepResult = env.step(action);
            total_return += result.reward;
            obs = result.obs;
            steps += 1;
        }

        // Extract terminal info
        let (placement, final_hp) = extract_terminal_info(&obs, total_return, max_rounds);

        episodes.push(EpisodeResult {
            seed,
            total_return,
            placement,
            steps,
            final_hp,
        });
    }

    let returns: Vec<f32> = episodes.iter().map(|e| e.total_return).collect();
    let placements: Vec<f32> = episodes.iter().map(|e| e.placement).collect();

    let (return_mean, return_std, return_min, return_max) = compute_stats(&returns);
    let (placement_mean, placement_std, placement_min, placement_max) = compute_stats(&placements);

    EvalReport {
        n_episodes: episodes.len(),
        return_mean,
        return_std,
        return_min,
        return_max,
        placement_mean,
        placement_std,
        placement_min,
        placement_max,
        episodes,
    }
}

/// Heuristic to extract placement from terminal state.
/// Since we don't have direct access to the env's internal placement,
/// estimate from the return: positive return ≈ top-4, negative ≈ bottom-4.
fn extract_terminal_info(_obs: &Obs, total_return: f32, _max_rounds: u8) -> (f32, u16) {
    // Approximate placement from total return sign/magnitude
    // Positive return → good placement (1-4), negative → bad (5-8)
    let placement = if total_return > 6.0 {
        1.0
    } else if total_return > 2.0 {
        2.0
    } else if total_return > 0.0 {
        3.0 + (2.0 - total_return).max(0.0)
    } else if total_return > -4.0 {
        5.0
    } else if total_return > -8.0 {
        6.0
    } else {
        7.0 + (-8.0 - total_return).min(1.0)
    };
    // Estimate final HP from return (crude: higher return → more HP remaining)
    let final_hp = ((total_return + 8.0) * 5.0).clamp(0.0, 100.0) as u16;
    (placement, final_hp)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_policy_runs() {
        let mut policy = RandomPolicy::new(42);
        let report = evaluate_policy(&mut policy, &[1, 2, 3, 4, 5], 6);
        assert_eq!(report.n_episodes, 5);
        // Return stats should be finite
        assert!(report.return_mean.is_finite());
        assert!(report.placement_mean.is_finite());
        println!("{report}");
    }

    #[test]
    fn test_rule_teacher_policy_runs() {
        let mut policy = RuleTeacherPolicy::new();
        let report = evaluate_policy(&mut policy, &[10, 20, 30], 6);
        assert_eq!(report.n_episodes, 3);
        assert!(report.return_mean.is_finite());
        assert!(report.placement_mean.is_finite());
        println!("{report}");
    }

    #[test]
    fn test_game_action_to_discrete_mapping() {
        assert_eq!(
            game_action_to_discrete(&GameAction::Noop { reason: "test".into() }),
            DiscreteAction::Noop
        );
        assert_eq!(
            game_action_to_discrete(&GameAction::BuyUnit { slot: 0 }),
            DiscreteAction::BuySlot0
        );
        assert_eq!(
            game_action_to_discrete(&GameAction::BuyUnit { slot: 3 }),
            DiscreteAction::BuySlot3
        );
        assert_eq!(
            game_action_to_discrete(&GameAction::BuyUnit { slot: 7 }),
            DiscreteAction::Noop
        );
        assert_eq!(
            game_action_to_discrete(&GameAction::Reroll),
            DiscreteAction::Reroll
        );
        assert_eq!(
            game_action_to_discrete(&GameAction::BuyXp),
            DiscreteAction::BuyXp
        );
        assert_eq!(
            game_action_to_discrete(&GameAction::SellUnit { unit_id: "x".into() }),
            DiscreteAction::SellWeakest
        );
        assert_eq!(
            game_action_to_discrete(&GameAction::ChooseAugment { index: 1 }),
            DiscreteAction::ChooseAugment1
        );
        assert_eq!(
            game_action_to_discrete(&GameAction::ChooseAugment { index: 5 }),
            DiscreteAction::Noop
        );
    }

    #[test]
    fn test_random_policy_uses_mask() {
        let mut policy = RandomPolicy::new(99);
        let obs = Obs {
            scalars: vec![0.0; 8],
            shop_costs: vec![0.0; 5],
            shop_preferred: vec![0.0; 5],
            board_cost_dist: vec![0.0; 5],
            phase: vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            flags: vec![0.0; 4],
        };
        // Only Noop is legal (35 actions total)
        let mut mask = vec![false; 35];
        mask[0] = true;
        let action = policy.select_action(&obs, &mask, None);
        assert_eq!(action, DiscreteAction::Noop);
    }

    #[test]
    fn test_eval_report_display() {
        let report = EvalReport {
            n_episodes: 10,
            return_mean: 1.5,
            return_std: 3.2,
            return_min: -5.0,
            return_max: 8.0,
            placement_mean: 3.5,
            placement_std: 1.8,
            placement_min: 1.0,
            placement_max: 7.0,
            episodes: vec![],
        };
        let display = format!("{report}");
        assert!(display.contains("10 episodes"));
        assert!(display.contains("Return"));
        assert!(display.contains("Placement"));
    }
}
