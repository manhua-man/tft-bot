use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use ort::{session::Session, value::Tensor};
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tft_domain::{
    augment_name_matches, first_open_board_slot, legal_actions, unit_name_matches, ActionPlan,
    AdviceItem, AdvicePayload, BoardPhase, BoardPosition, GameAction, GameSnapshot, ModelMetadata,
    TemplateMatchReadout, UnitInstance,
};

pub trait StrategyKernel {
    fn id(&self) -> &str;
    fn plan(&self, snapshot: &GameSnapshot) -> ActionPlan;
}

pub trait AdvisorKernel {
    fn id(&self) -> &str;
    fn advise(&self, snapshot: &GameSnapshot) -> AdvicePayload;
}

pub trait PhaseStrategy {
    fn decide_lobby(&self, snapshot: &GameSnapshot) -> ActionPlan;
    fn decide_augment(&self, snapshot: &GameSnapshot) -> ActionPlan;
    fn decide_shop_economy(&self, snapshot: &GameSnapshot) -> ActionPlan;
    fn decide_board_placement(&self, snapshot: &GameSnapshot) -> ActionPlan;
    fn decide_itemization(&self, snapshot: &GameSnapshot) -> ActionPlan;
    fn decide_carousel(&self, snapshot: &GameSnapshot) -> ActionPlan;
}

pub struct PhaseRouter<K> {
    inner: K,
    router_id: String,
}

impl<K> PhaseRouter<K> {
    pub fn new(inner: K) -> Self {
        let router_id = std::any::type_name::<K>().to_string();
        Self { inner, router_id }
    }
}

impl<K> StrategyKernel for PhaseRouter<K>
where
    K: PhaseStrategy,
{
    fn id(&self) -> &str {
        &self.router_id
    }

    fn plan(&self, snapshot: &GameSnapshot) -> ActionPlan {
        match snapshot.board_phase {
            BoardPhase::Lobby => self.inner.decide_lobby(snapshot),
            BoardPhase::Augment => self.inner.decide_augment(snapshot),
            BoardPhase::ShopEconomy => {
                let mut primary = self.inner.decide_shop_economy(snapshot);
                let itemization = self.inner.decide_itemization(snapshot);
                merge_itemization_into_shop_plan(&mut primary, itemization);
                primary
            }
            BoardPhase::BoardPlacement | BoardPhase::Combat | BoardPhase::PostCombat => {
                self.inner.decide_board_placement(snapshot)
            }
            BoardPhase::Carousel => self.inner.decide_carousel(snapshot),
        }
    }
}

fn merge_itemization_into_shop_plan(primary: &mut ActionPlan, itemization: ActionPlan) {
    let primary_has_equip = primary
        .actions
        .iter()
        .any(|action| matches!(action, GameAction::EquipItem { .. }));
    if primary_has_equip {
        return;
    }

    let item_actions = itemization
        .actions
        .into_iter()
        .filter(|action| !matches!(action, GameAction::Noop { .. }))
        .collect::<Vec<_>>();
    if item_actions.is_empty() {
        return;
    }

    // Keep this conservative: only append itemization when the primary shop plan is holding or
    // purely making room (sell). This avoids turning a single "best action" policy into
    // accidental multi-step chains.
    let primary_is_hold = primary
        .actions
        .first()
        .is_some_and(|action| matches!(action, GameAction::Noop { .. }));
    let primary_is_sell = primary
        .actions
        .first()
        .is_some_and(|action| matches!(action, GameAction::SellUnit { .. }));
    if !primary_is_hold && !primary_is_sell {
        return;
    }

    let mut next_actions = Vec::with_capacity(item_actions.len() + primary.actions.len());
    next_actions.extend(item_actions);
    next_actions.append(&mut primary.actions);
    primary.actions = next_actions;
    primary.summary = format!("{} + {}", itemization.summary, primary.summary);
    primary.confidence = (primary.confidence * 0.92).clamp(0.1, 0.95);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleKernel {
    pub kernel_id: String,
}

impl Default for RuleKernel {
    fn default() -> Self {
        Self {
            kernel_id: "rule-baseline".into(),
        }
    }
}

fn normalize_match_token(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_alphanumeric() || *ch > '\u{4E00}')
        .collect::<String>()
        .to_lowercase()
}

fn fuzzy_token_matches(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    if left.is_empty() || right.is_empty() {
        return false;
    }

    if left.eq_ignore_ascii_case(right) || left.contains(right) || right.contains(left) {
        return true;
    }

    let ln = normalize_match_token(left);
    let rn = normalize_match_token(right);
    !ln.is_empty() && !rn.is_empty() && (ln == rn || ln.contains(&rn) || rn.contains(&ln))
}

fn list_contains_fuzzy(values: &[String], candidate: &str) -> bool {
    values
        .iter()
        .any(|value| fuzzy_token_matches(value, candidate))
}

fn list_contains_unit_alias(values: &[String], candidate: &str) -> bool {
    values
        .iter()
        .any(|value| unit_name_matches(value, candidate))
}

fn list_contains_augment_alias(values: &[String], candidate: &str) -> bool {
    values
        .iter()
        .any(|value| augment_name_matches(value, candidate))
}

impl RuleKernel {
    fn preferred_shop_slot(&self, snapshot: &GameSnapshot) -> Option<u8> {
        snapshot
            .user_preset
            .desired_units
            .iter()
            .find_map(|desired| {
                snapshot
                    .shop
                    .iter()
                    .find(|slot| unit_name_matches(&slot.unit_name, desired))
                    .map(|slot| slot.index)
            })
    }

    fn stage_major(snapshot: &GameSnapshot) -> Option<u8> {
        snapshot
            .stage
            .split('-')
            .next()
            .and_then(|value| value.trim().parse::<u8>().ok())
            .filter(|value| *value > 0)
    }

    fn desired_unit_count_on_roster(snapshot: &GameSnapshot) -> usize {
        snapshot
            .board
            .iter()
            .chain(&snapshot.bench)
            .filter(|unit| {
                list_contains_unit_alias(&snapshot.user_preset.desired_units, &unit.name)
            })
            .count()
    }

    fn bench_unit_to_deploy<'a>(&self, snapshot: &'a GameSnapshot) -> Option<&'a UnitInstance> {
        snapshot
            .user_preset
            .desired_units
            .iter()
            .find_map(|desired| {
                snapshot
                    .bench
                    .iter()
                    .find(|unit| unit_name_matches(&unit.name, desired))
            })
            .or_else(|| {
                snapshot.bench.iter().max_by(|left, right| {
                    (left.stars, left.items.len(), left.cost, left.name.as_str()).cmp(&(
                        right.stars,
                        right.items.len(),
                        right.cost,
                        right.name.as_str(),
                    ))
                })
            })
    }

    fn equip_candidate<'a>(
        &self,
        snapshot: &'a GameSnapshot,
    ) -> Option<(&'a UnitInstance, &'a TemplateMatchReadout)> {
        if snapshot.items.is_empty() {
            return None;
        }

        let unit = snapshot
            .board
            .iter()
            .chain(&snapshot.bench)
            .find(|unit| {
                unit.items.len() < 3
                    && list_contains_unit_alias(&snapshot.user_preset.desired_units, &unit.name)
            })
            .or_else(|| {
                snapshot
                    .board
                    .iter()
                    .chain(&snapshot.bench)
                    .find(|unit| unit.items.len() < 3)
            })?;

        let preferred_item = snapshot
            .items
            .iter()
            .find(|item| list_contains_fuzzy(&snapshot.user_preset.item_priority, &item.value))
            .or_else(|| snapshot.items.first());

        preferred_item.map(|item| (unit, item))
    }

    fn sell_candidate<'a>(&self, snapshot: &'a GameSnapshot) -> Option<&'a UnitInstance> {
        snapshot
            .bench
            .iter()
            .filter(|unit| {
                !list_contains_unit_alias(&snapshot.user_preset.desired_units, &unit.name)
            })
            .min_by(|left, right| {
                (left.stars, left.cost, left.name.as_str()).cmp(&(
                    right.stars,
                    right.cost,
                    right.name.as_str(),
                ))
            })
            .or_else(|| {
                snapshot
                    .bench
                    .iter()
                    .min_by_key(|unit| (unit.stars, unit.cost))
            })
    }
}

impl PhaseStrategy for RuleKernel {
    fn decide_lobby(&self, snapshot: &GameSnapshot) -> ActionPlan {
        if snapshot.flags.ready_check_active {
            ActionPlan {
                kernel_id: self.kernel_id.clone(),
                summary: "Accept ready check".into(),
                confidence: 0.95,
                actions: vec![GameAction::QueueAccept],
            }
        } else {
            ActionPlan::noop(self.kernel_id.clone(), "waiting in lobby")
        }
    }

    fn decide_augment(&self, snapshot: &GameSnapshot) -> ActionPlan {
        let index = snapshot
            .augments
            .iter()
            .position(|name| {
                list_contains_augment_alias(&snapshot.user_preset.augment_priority, name)
            })
            .unwrap_or(0) as u8;

        ActionPlan {
            kernel_id: self.kernel_id.clone(),
            summary: "Take preferred augment".into(),
            confidence: 0.8,
            actions: vec![GameAction::ChooseAugment { index }],
        }
    }

    fn decide_shop_economy(&self, snapshot: &GameSnapshot) -> ActionPlan {
        let stage_major = Self::stage_major(snapshot).unwrap_or(0);
        let desired_units_owned = Self::desired_unit_count_on_roster(snapshot);
        let board_gap = snapshot.level.saturating_sub(snapshot.board.len() as u8);

        if snapshot.flags.bench_full {
            if let Some(slot) = self.preferred_shop_slot(snapshot) {
                if let Some(candidate) = self.sell_candidate(snapshot) {
                    return ActionPlan {
                        kernel_id: self.kernel_id.clone(),
                        summary: format!(
                            "Make room then buy shop slot {slot} ({})",
                            snapshot
                                .shop
                                .iter()
                                .find(|entry| entry.index == slot)
                                .map(|entry| entry.unit_name.as_str())
                                .unwrap_or("?")
                        ),
                        confidence: 0.62,
                        actions: vec![
                            GameAction::SellUnit {
                                unit_id: candidate.id.clone(),
                            },
                            GameAction::BuyUnit { slot },
                        ],
                    };
                }
            }
        }

        if let Some(slot) = self.preferred_shop_slot(snapshot) {
            return ActionPlan {
                kernel_id: self.kernel_id.clone(),
                summary: "Buy preferred unit".into(),
                confidence: 0.75,
                actions: vec![GameAction::BuyUnit { slot }],
            };
        }

        if snapshot.flags.can_level
            && (snapshot.gold >= 50
                || (stage_major >= 4
                    && snapshot.gold >= 20
                    && (desired_units_owned > 0 || snapshot.streak >= 2 || board_gap <= 1))
                || (stage_major >= 3 && snapshot.gold >= 32 && desired_units_owned >= 2))
        {
            return ActionPlan {
                kernel_id: self.kernel_id.clone(),
                summary: "Convert excess gold into level tempo".into(),
                confidence: 0.6,
                actions: vec![GameAction::BuyXp],
            };
        }

        let reroll_floor = if stage_major >= 4 { 6 } else { 10 };
        if snapshot.flags.can_reroll
            && stage_major >= 3
            && snapshot.gold >= reroll_floor
            && self.preferred_shop_slot(snapshot).is_none()
            && (snapshot.flags.bench_full || desired_units_owned == 0 || board_gap == 0)
        {
            return ActionPlan {
                kernel_id: self.kernel_id.clone(),
                summary: "Search for a preferred board upgrade".into(),
                confidence: 0.5,
                actions: vec![GameAction::Reroll],
            };
        }

        if desired_units_owned > 0
            && snapshot.gold >= 4
            && snapshot.flags.can_level
            && board_gap > 0
        {
            return ActionPlan {
                kernel_id: self.kernel_id.clone(),
                summary: "Preserve tempo while holding a board gap".into(),
                confidence: 0.52,
                actions: vec![GameAction::BuyXp],
            };
        }

        ActionPlan::noop(self.kernel_id.clone(), "hold economy")
    }

    fn decide_board_placement(&self, snapshot: &GameSnapshot) -> ActionPlan {
        if snapshot.board.len() >= snapshot.level as usize {
            return ActionPlan::noop(self.kernel_id.clone(), "board already filled");
        }
        let Some(target) = first_open_board_slot(snapshot) else {
            return ActionPlan::noop(self.kernel_id.clone(), "no open board slot");
        };
        let Some(unit) = self.bench_unit_to_deploy(snapshot) else {
            return ActionPlan::noop(self.kernel_id.clone(), "bench is empty");
        };

        let preferred = list_contains_unit_alias(&snapshot.user_preset.desired_units, &unit.name);
        ActionPlan {
            kernel_id: self.kernel_id.clone(),
            summary: format!("Deploy {} to board", unit.name),
            confidence: if preferred { 0.78 } else { 0.6 },
            actions: vec![GameAction::MoveBoard {
                unit_id: unit.id.clone(),
                to: target,
            }],
        }
    }

    fn decide_itemization(&self, snapshot: &GameSnapshot) -> ActionPlan {
        let Some((unit, item_id)) = self.equip_candidate(snapshot) else {
            return ActionPlan::noop(self.kernel_id.clone(), "no itemization action");
        };

        ActionPlan {
            kernel_id: self.kernel_id.clone(),
            summary: format!("Equip {} on {}", item_id.value, unit.name),
            confidence: 0.62,
            actions: vec![GameAction::EquipItem {
                unit_id: unit.id.clone(),
                item_id: item_id.clone(),
            }],
        }
    }

    fn decide_carousel(&self, snapshot: &GameSnapshot) -> ActionPlan {
        let target = snapshot
            .user_preset
            .desired_units
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".into());
        ActionPlan {
            kernel_id: self.kernel_id.clone(),
            summary: format!("Carousel: hold (target {target})"),
            confidence: 0.9,
            actions: vec![GameAction::Noop {
                reason: "carousel_hold".into(),
            }],
        }
    }
}

const POLICY_FEATURE_COUNT: usize = 18;
const POLICY_FEATURE_COUNT_V2: usize = 50;
const POLICY_ACTION_COUNT: usize = 6;

const DEFAULT_PHASE3_GUARD_PATH: &str = "configs/phase3-guard.json";
const DEFAULT_MODEL_METADATA_PATH: &str = "artifacts/models/model-metadata-v2.json";
const LEGACY_MODEL_METADATA_PATH: &str = "artifacts/models/model-metadata.json";
const MACRO_ACTION_LABELS: [&str; 6] = ["hold", "econ", "roll", "level", "stabilize", "all_in"];
const SHOP_ACTION_LABELS: [&str; 8] = [
    "hold",
    "buy_slot_0",
    "buy_slot_1",
    "buy_slot_2",
    "buy_slot_3",
    "buy_slot_4",
    "reroll",
    "lock",
];
const BOARD_ACTION_LABELS: [&str; 5] = [
    "hold",
    "promote_best",
    "swap_front_back",
    "fill_board",
    "sell_low_value",
];
const AUGMENT_ACTION_LABELS: [&str; 4] = ["hold", "pick_1", "pick_2", "pick_3"];

#[derive(Debug, Clone, Copy, Default)]
pub struct Phase3Guard {
    pub training_enabled: bool,
    pub inference_enabled: bool,
    pub default_switch_enabled: bool,
}

impl Phase3Guard {
    pub fn frozen(&self) -> bool {
        !(self.training_enabled || self.inference_enabled || self.default_switch_enabled)
    }
}

pub fn load_phase3_guard(path: impl AsRef<Path>) -> Phase3Guard {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return Phase3Guard::default(),
    };
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(_) => return Phase3Guard::default(),
    };

    let phase3 = parsed
        .get("phase3")
        .and_then(|value| value.as_object())
        .cloned()
        .unwrap_or_else(|| parsed.as_object().cloned().unwrap_or_default());

    let read_bool = |key: &str| -> bool {
        phase3
            .get(key)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    };

    Phase3Guard {
        training_enabled: read_bool("training_enabled"),
        inference_enabled: read_bool("inference_enabled"),
        default_switch_enabled: read_bool("default_switch_enabled"),
    }
}

pub fn phase3_default_switch_enabled() -> bool {
    load_phase3_guard(DEFAULT_PHASE3_GUARD_PATH).default_switch_enabled
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnedPolicyAction {
    Hold,
    BuyUnit,
    BuyXp,
    Reroll,
    MoveBoard,
    ChooseAugment,
}

impl LearnedPolicyAction {
    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::BuyUnit,
            2 => Self::BuyXp,
            3 => Self::Reroll,
            4 => Self::MoveBoard,
            5 => Self::ChooseAugment,
            _ => Self::Hold,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::BuyUnit => "buy_unit",
            Self::BuyXp => "buy_xp",
            Self::Reroll => "reroll",
            Self::MoveBoard => "move_board",
            Self::ChooseAugment => "choose_augment",
        }
    }

    fn is_valid_for_phase(self, phase: &BoardPhase) -> bool {
        match phase {
            BoardPhase::ShopEconomy => matches!(
                self,
                Self::Hold | Self::BuyUnit | Self::BuyXp | Self::Reroll
            ),
            BoardPhase::BoardPlacement | BoardPhase::Combat | BoardPhase::PostCombat => {
                matches!(self, Self::Hold | Self::MoveBoard)
            }
            BoardPhase::Augment => matches!(self, Self::Hold | Self::ChooseAugment),
            BoardPhase::Lobby | BoardPhase::Carousel => matches!(self, Self::Hold),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedInferenceMode {
    Strict,
    Guarded,
}

impl LearnedInferenceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Guarded => "guarded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedDecisionSource {
    PriorOnly,
    LearnedResidual,
    FallbackRule,
    StrictNoop,
}

impl LearnedDecisionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PriorOnly => "prior_only",
            Self::LearnedResidual => "learned_residual",
            Self::FallbackRule => "fallback_rule",
            Self::StrictNoop => "strict_noop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedFallbackReason {
    ModelMissing,
    OnnxLoadFailed,
    RuntimeError,
    InvalidPhaseAction,
}

impl LearnedFallbackReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModelMissing => "model_missing",
            Self::OnnxLoadFailed => "onnx_load_failed",
            Self::RuntimeError => "runtime_error",
            Self::InvalidPhaseAction => "invalid_phase_action",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PredictFailureReason {
    ModelMissing,
    OnnxLoadFailed,
    RuntimeError,
}

impl PredictFailureReason {
    fn as_fallback_reason(self) -> LearnedFallbackReason {
        match self {
            Self::ModelMissing => LearnedFallbackReason::ModelMissing,
            Self::OnnxLoadFailed => LearnedFallbackReason::OnnxLoadFailed,
            Self::RuntimeError => LearnedFallbackReason::RuntimeError,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LearnedDecisionTrace {
    pub action: LearnedPolicyAction,
    pub prior_action: LearnedPolicyAction,
    pub decision_source: LearnedDecisionSource,
    pub fallback_reason: Option<LearnedFallbackReason>,
}

const PRIOR_BIAS_STRENGTH: f32 = 2.0;

#[derive(Debug)]
struct LearnedPolicyRuntime {
    model: ModelMetadata,
    session: Mutex<Option<Session>>,
    is_multi_head: bool,
}

impl LearnedPolicyRuntime {
    fn new(model: ModelMetadata) -> Self {
        let is_multi_head = model.id.eq_ignore_ascii_case("learned-v2")
            || model.onnx_path.contains("learned-v2")
            || detect_multi_head_metadata(DEFAULT_MODEL_METADATA_PATH)
            || detect_multi_head_metadata(LEGACY_MODEL_METADATA_PATH);
        Self {
            model,
            session: Mutex::new(None),
            is_multi_head,
        }
    }

    fn predict(
        &self,
        snapshot: &GameSnapshot,
        prior: &LearnedKernelPrior,
    ) -> Result<LearnedPolicyAction, PredictFailureReason> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| PredictFailureReason::RuntimeError)?;
        if session.is_none() {
            if !Path::new(&self.model.onnx_path).exists() {
                return Err(PredictFailureReason::ModelMissing);
            }
            let loaded = Session::builder()
                .map_err(|_| PredictFailureReason::OnnxLoadFailed)?
                .commit_from_file(&self.model.onnx_path)
                .map_err(|_| PredictFailureReason::OnnxLoadFailed)?;
            *session = Some(loaded);
        }

        let features: Vec<f32> = if self.is_multi_head {
            policy_features_v2(snapshot).to_vec()
        } else {
            policy_features(snapshot).to_vec()
        };
        let input = Tensor::from_array(([1usize, features.len()], features.into_boxed_slice()))
            .map_err(|_| PredictFailureReason::RuntimeError)?;
        let outputs = session
            .as_mut()
            .ok_or(PredictFailureReason::RuntimeError)?
            .run(ort::inputs![input])
            .map_err(|_| PredictFailureReason::RuntimeError)?;

        if self.is_multi_head {
            let macro_best = best_output_index_with_bias(
                &outputs,
                0,
                Some(prior.macro_index),
                PRIOR_BIAS_STRENGTH,
            )
            .ok_or(PredictFailureReason::RuntimeError)?;
            let shop_best = best_output_index_with_bias(
                &outputs,
                1,
                Some(prior.shop_index),
                PRIOR_BIAS_STRENGTH,
            )
            .ok_or(PredictFailureReason::RuntimeError)?;
            let board_best = best_output_index_with_bias(
                &outputs,
                2,
                Some(prior.board_index),
                PRIOR_BIAS_STRENGTH,
            )
            .ok_or(PredictFailureReason::RuntimeError)?;
            let augment_best = best_output_index_with_bias(
                &outputs,
                4,
                Some(prior.augment_index),
                PRIOR_BIAS_STRENGTH,
            )
            .ok_or(PredictFailureReason::RuntimeError)?;
            return Ok(project_multi_head_action(
                snapshot,
                macro_best,
                shop_best,
                board_best,
                augment_best,
            ));
        }

        let (_, logits) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|_| PredictFailureReason::RuntimeError)?;
        let best_index = logits
            .iter()
            .take(POLICY_ACTION_COUNT)
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(right.1))
            .map(|(index, _)| index)
            .ok_or(PredictFailureReason::RuntimeError)?;
        Ok(LearnedPolicyAction::from_index(best_index))
    }
}

fn clamp_ratio(value: f32, maximum: f32) -> f32 {
    if maximum <= 0.0 {
        return 0.0;
    }
    (value / maximum).clamp(0.0, 1.0)
}

fn preferred_shop_hit(snapshot: &GameSnapshot) -> bool {
    snapshot
        .shop
        .iter()
        .any(|slot| list_contains_unit_alias(&snapshot.user_preset.desired_units, &slot.unit_name))
}

fn phase_features(snapshot: &GameSnapshot) -> (f32, f32, f32) {
    match snapshot.board_phase {
        BoardPhase::ShopEconomy => (1.0, 0.0, 0.0),
        BoardPhase::BoardPlacement | BoardPhase::Combat | BoardPhase::PostCombat => (0.0, 1.0, 0.0),
        BoardPhase::Augment => (0.0, 0.0, 1.0),
        BoardPhase::Lobby | BoardPhase::Carousel => (0.0, 0.0, 0.0),
    }
}

fn detect_multi_head_metadata(path: &str) -> bool {
    let Ok(raw) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    value
        .get("modelHeads")
        .and_then(|v| v.as_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("multi"))
        || value.get("heads").is_some_and(|v| v.is_object())
}

fn best_output_index_with_bias(
    outputs: &ort::session::SessionOutputs<'_>,
    index: usize,
    bias_index: Option<usize>,
    bias_strength: f32,
) -> Option<usize> {
    if index >= outputs.len() {
        return None;
    }
    let (_, logits) = outputs[index].try_extract_tensor::<f32>().ok()?;
    logits
        .iter()
        .enumerate()
        .map(|(candidate_index, value)| {
            let bias = if bias_index.is_some_and(|target| target == candidate_index) {
                bias_strength
            } else {
                0.0
            };
            (candidate_index, *value + bias)
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(candidate_index, _)| candidate_index)
}

fn project_multi_head_action(
    snapshot: &GameSnapshot,
    macro_best: usize,
    shop_best: usize,
    board_best: usize,
    augment_best: usize,
) -> LearnedPolicyAction {
    match snapshot.board_phase {
        BoardPhase::ShopEconomy => {
            match SHOP_ACTION_LABELS.get(shop_best).copied().unwrap_or("hold") {
                "buy_slot_0" | "buy_slot_1" | "buy_slot_2" | "buy_slot_3" | "buy_slot_4" => {
                    LearnedPolicyAction::BuyUnit
                }
                "reroll" => LearnedPolicyAction::Reroll,
                _ => match MACRO_ACTION_LABELS
                    .get(macro_best)
                    .copied()
                    .unwrap_or("hold")
                {
                    "level" => LearnedPolicyAction::BuyXp,
                    "roll" | "all_in" => LearnedPolicyAction::Reroll,
                    "econ" if preferred_shop_hit(snapshot) => LearnedPolicyAction::BuyUnit,
                    _ => LearnedPolicyAction::Hold,
                },
            }
        }
        BoardPhase::BoardPlacement | BoardPhase::Combat | BoardPhase::PostCombat => {
            match BOARD_ACTION_LABELS
                .get(board_best)
                .copied()
                .unwrap_or("hold")
            {
                "promote_best" | "swap_front_back" | "fill_board" => LearnedPolicyAction::MoveBoard,
                _ => LearnedPolicyAction::Hold,
            }
        }
        BoardPhase::Augment => match AUGMENT_ACTION_LABELS
            .get(augment_best)
            .copied()
            .unwrap_or("hold")
        {
            "pick_1" | "pick_2" | "pick_3" => LearnedPolicyAction::ChooseAugment,
            _ => LearnedPolicyAction::Hold,
        },
        BoardPhase::Lobby | BoardPhase::Carousel => LearnedPolicyAction::Hold,
    }
}

fn policy_features(snapshot: &GameSnapshot) -> [f32; POLICY_FEATURE_COUNT] {
    let board_units = snapshot.board.len() as f32;
    let bench_units = snapshot.bench.len() as f32;
    let shop_units = snapshot.shop.len() as f32;
    let level = snapshot.level.max(1) as f32;
    let board_gap = (level - board_units).max(0.0);
    let augment_count = snapshot.augments.len() as f32;
    let (phase_shop, phase_board, phase_augment) = phase_features(snapshot);

    [
        clamp_ratio(snapshot.gold as f32, 60.0),
        clamp_ratio(level, 10.0),
        clamp_ratio(board_units, 10.0),
        clamp_ratio(bench_units, 9.0),
        clamp_ratio(shop_units, 5.0),
        clamp_ratio(snapshot.items.len() as f32, 10.0),
        clamp_ratio(board_gap, 10.0),
        clamp_ratio(snapshot.health as f32, 100.0),
        (snapshot.streak as f32 / 10.0).clamp(-1.0, 1.0),
        f32::from(preferred_shop_hit(snapshot)),
        f32::from(snapshot.flags.ready_check_active),
        f32::from(snapshot.flags.can_level),
        f32::from(snapshot.flags.can_reroll),
        f32::from(snapshot.flags.bench_full),
        clamp_ratio(augment_count, 3.0),
        phase_shop,
        phase_board,
        phase_augment,
    ]
}

fn clamp_ratio_v2(value: f32, maximum: f32) -> f32 {
    if maximum <= 0.0 {
        return 0.0;
    }
    (value / maximum).clamp(0.0, 1.0)
}

fn parse_stage(snapshot: &GameSnapshot) -> (f32, f32, f32) {
    let mut major = 0.0;
    let mut minor = 0.0;
    if let Some((major_raw, minor_raw)) = snapshot.stage.trim().split_once('-') {
        if let Ok(parsed) = major_raw.trim().parse::<f32>() {
            major = parsed;
        }
        if let Ok(parsed) = minor_raw.trim().parse::<f32>() {
            minor = parsed;
        }
    }
    let round_norm = (minor * 1.5).clamp(0.0, 10.0) / 10.0;
    (major, minor, round_norm)
}

fn policy_features_v2(snapshot: &GameSnapshot) -> [f32; POLICY_FEATURE_COUNT_V2] {
    let gold = snapshot.gold as f32;
    let level = snapshot.level.max(1) as f32;
    let health = snapshot.health as f32;
    let board_units = snapshot.board.len() as f32;
    let bench_units = snapshot.bench.len() as f32;
    let shop_units = snapshot.shop.len() as f32;
    let item_count = snapshot.items.len() as f32;
    let augment_count = snapshot.augments.len() as f32;
    let streak = snapshot.streak as f32;
    let (major, minor, round_norm) = parse_stage(snapshot);
    let phase = match snapshot.board_phase {
        BoardPhase::ShopEconomy => "shopeconomy",
        BoardPhase::BoardPlacement | BoardPhase::Combat | BoardPhase::PostCombat => "board",
        BoardPhase::Augment => "augment",
        BoardPhase::Lobby | BoardPhase::Carousel => "other",
    };

    let interest = (gold * 0.1).clamp(0.0, 5.0) / 5.0;
    let econ_tier = (level / 11.0).clamp(0.0, 1.0);
    let gold_per_10 = (gold / 10.0).clamp(0.0, 8.0) / 8.0;
    let avg_damage = if health < 100.0 {
        (100.0 - health) / health.max(1.0)
    } else {
        0.0
    };
    let hp_ratio = (health / 100.0).clamp(0.0, 1.0);
    let board_fill = board_units / level.max(1.0);
    let bench_pressure = bench_units / 9.0;
    let board_bench_ratio = board_units / bench_units.max(1.0);
    let items_per_unit = item_count / board_units.max(1.0);
    let item_density = (item_count / 12.0).clamp(0.0, 1.0);
    let item_tankness = ((item_count / board_units.max(1.0)) * 0.5).clamp(0.0, 1.0);
    let total_cost_est = (level * 1.5).clamp(0.0, 27.0) / 27.0;
    let bench_util = bench_units / 9.0;
    let bench_cap = (bench_units / level.max(1.0)).clamp(0.0, 1.0);
    let near_pvp = f32::from(major >= 3.0 && minor >= 3.0);
    let hp_stress = f32::from(hp_ratio < 0.4);
    let loss_streak = f32::from(snapshot.streak < -2);
    let win_streak = f32::from(snapshot.streak > 2);

    [
        clamp_ratio_v2(gold, 80.0),
        clamp_ratio_v2(level, 11.0),
        clamp_ratio_v2(health, 100.0),
        clamp_ratio_v2(board_units, 10.0),
        clamp_ratio_v2(bench_units, 9.0),
        clamp_ratio_v2(shop_units, 5.0),
        clamp_ratio_v2(item_count, 12.0),
        clamp_ratio_v2(augment_count, 3.0),
        clamp_ratio_v2(streak, 12.0),
        f32::from(snapshot.flags.can_level),
        f32::from(snapshot.flags.can_reroll),
        f32::from(snapshot.flags.bench_full),
        f32::from(phase == "shopeconomy"),
        f32::from(phase == "board"),
        f32::from(phase == "augment"),
        0.0,
        1.0,
        0.0,
        0.0,
        f32::from(snapshot.patch != "unknown"),
        (major / 5.0).clamp(0.0, 1.0),
        (minor / 5.0).clamp(0.0, 1.0),
        round_norm,
        interest,
        econ_tier,
        gold_per_10,
        avg_damage.clamp(0.0, 5.0) / 5.0,
        hp_ratio,
        board_fill.clamp(0.0, 1.0),
        bench_pressure.clamp(0.0, 1.0),
        board_bench_ratio.clamp(0.0, 1.0),
        f32::from(augment_count >= 1.0),
        f32::from(augment_count >= 2.0),
        f32::from(augment_count >= 3.0),
        (items_per_unit.clamp(0.0, 3.0)) / 3.0,
        item_density,
        item_tankness,
        total_cost_est,
        bench_util.clamp(0.0, 1.0),
        bench_cap,
        near_pvp,
        hp_stress,
        loss_streak,
        win_streak,
        1.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
    ]
}

#[derive(Debug, Clone)]
pub struct LearnedKernel {
    pub kernel_id: String,
    pub model: ModelMetadata,
    runtime: Arc<LearnedPolicyRuntime>,
    mode: LearnedInferenceMode,
    residual_enabled: bool,
}

#[derive(Debug, Clone, Copy)]
struct LearnedKernelPrior {
    action: LearnedPolicyAction,
    macro_index: usize,
    shop_index: usize,
    board_index: usize,
    augment_index: usize,
}

impl LearnedKernel {
    pub fn new(model: ModelMetadata) -> Self {
        Self::new_with_mode(model, LearnedInferenceMode::Guarded)
    }

    pub fn new_with_mode(model: ModelMetadata, mode: LearnedInferenceMode) -> Self {
        Self::with_options(model, mode, true)
    }

    pub fn new_prior_only(model: ModelMetadata, mode: LearnedInferenceMode) -> Self {
        Self::with_options(model, mode, false)
    }

    fn with_options(
        model: ModelMetadata,
        mode: LearnedInferenceMode,
        residual_enabled: bool,
    ) -> Self {
        Self {
            kernel_id: "learned-policy".into(),
            runtime: Arc::new(LearnedPolicyRuntime::new(model.clone())),
            model,
            mode,
            residual_enabled,
        }
    }

    fn best_slot_by_model(&self, snapshot: &GameSnapshot) -> Option<u8> {
        snapshot
            .shop
            .iter()
            .max_by_key(|slot| {
                let preferred =
                    list_contains_unit_alias(&snapshot.user_preset.desired_units, &slot.unit_name)
                        as i32;
                (preferred * 10) + slot.cost as i32
            })
            .map(|slot| slot.index)
    }

    fn preferred_augment_index(&self, snapshot: &GameSnapshot) -> u8 {
        snapshot
            .augments
            .iter()
            .position(|augment| {
                list_contains_augment_alias(&snapshot.user_preset.augment_priority, augment)
            })
            .unwrap_or(0) as u8
    }

    fn board_move_candidate(&self, snapshot: &GameSnapshot) -> Option<(String, BoardPosition)> {
        if snapshot.board.len() >= snapshot.level as usize {
            return None;
        }

        let target = first_open_board_slot(snapshot)?;
        snapshot.bench.first().map(|unit| (unit.id.clone(), target))
    }

    fn fallback_shop_action(&self, snapshot: &GameSnapshot) -> LearnedPolicyAction {
        if preferred_shop_hit(snapshot) && self.best_slot_by_model(snapshot).is_some() {
            LearnedPolicyAction::BuyUnit
        } else if snapshot.gold >= 50 && snapshot.flags.can_level {
            LearnedPolicyAction::BuyXp
        } else if snapshot.gold >= 6 && snapshot.flags.can_reroll {
            LearnedPolicyAction::Reroll
        } else {
            LearnedPolicyAction::Hold
        }
    }

    fn fallback_board_action(&self, snapshot: &GameSnapshot) -> LearnedPolicyAction {
        if self.board_move_candidate(snapshot).is_some() {
            LearnedPolicyAction::MoveBoard
        } else {
            LearnedPolicyAction::Hold
        }
    }

    fn fallback_augment_action(&self, snapshot: &GameSnapshot) -> LearnedPolicyAction {
        if snapshot.augments.is_empty() {
            LearnedPolicyAction::Hold
        } else {
            LearnedPolicyAction::ChooseAugment
        }
    }

    fn fallback_phase_action(&self, snapshot: &GameSnapshot) -> LearnedPolicyAction {
        match snapshot.board_phase {
            BoardPhase::ShopEconomy => self.fallback_shop_action(snapshot),
            BoardPhase::BoardPlacement | BoardPhase::Combat | BoardPhase::PostCombat => {
                self.fallback_board_action(snapshot)
            }
            BoardPhase::Augment => self.fallback_augment_action(snapshot),
            BoardPhase::Lobby | BoardPhase::Carousel => LearnedPolicyAction::Hold,
        }
    }

    fn prior_for_snapshot(&self, snapshot: &GameSnapshot) -> LearnedKernelPrior {
        let action = self.fallback_phase_action(snapshot);
        let macro_index = match action {
            LearnedPolicyAction::BuyXp => 3,
            LearnedPolicyAction::Reroll => 2,
            LearnedPolicyAction::BuyUnit => 1,
            _ => 0,
        };
        let shop_index = match action {
            LearnedPolicyAction::BuyUnit => self
                .best_slot_by_model(snapshot)
                .map(|slot| usize::from(slot) + 1)
                .unwrap_or(1),
            LearnedPolicyAction::Reroll => 6,
            _ => 0,
        };
        let board_index = if matches!(action, LearnedPolicyAction::MoveBoard) {
            3
        } else {
            0
        };
        let augment_index = if matches!(action, LearnedPolicyAction::ChooseAugment) {
            1
        } else {
            0
        };
        LearnedKernelPrior {
            action,
            macro_index,
            shop_index,
            board_index,
            augment_index,
        }
    }

    fn trace_for_action(
        &self,
        snapshot: &GameSnapshot,
        prior: LearnedKernelPrior,
        action: LearnedPolicyAction,
    ) -> LearnedDecisionTrace {
        if !action.is_valid_for_phase(&snapshot.board_phase) {
            return match self.mode {
                LearnedInferenceMode::Strict => LearnedDecisionTrace {
                    action: LearnedPolicyAction::Hold,
                    prior_action: prior.action,
                    decision_source: LearnedDecisionSource::StrictNoop,
                    fallback_reason: Some(LearnedFallbackReason::InvalidPhaseAction),
                },
                LearnedInferenceMode::Guarded => LearnedDecisionTrace {
                    action: prior.action,
                    prior_action: prior.action,
                    decision_source: LearnedDecisionSource::FallbackRule,
                    fallback_reason: Some(LearnedFallbackReason::InvalidPhaseAction),
                },
            };
        }

        LearnedDecisionTrace {
            action,
            prior_action: prior.action,
            decision_source: if action == prior.action {
                LearnedDecisionSource::PriorOnly
            } else {
                LearnedDecisionSource::LearnedResidual
            },
            fallback_reason: None,
        }
    }

    pub fn decision_trace(&self, snapshot: &GameSnapshot) -> LearnedDecisionTrace {
        let prior = self.prior_for_snapshot(snapshot);
        if !self.residual_enabled {
            return LearnedDecisionTrace {
                action: prior.action,
                prior_action: prior.action,
                decision_source: LearnedDecisionSource::PriorOnly,
                fallback_reason: None,
            };
        }

        match self.runtime.predict(snapshot, &prior) {
            Ok(action) => self.trace_for_action(snapshot, prior, action),
            Err(reason) => match self.mode {
                LearnedInferenceMode::Strict => LearnedDecisionTrace {
                    action: LearnedPolicyAction::Hold,
                    prior_action: prior.action,
                    decision_source: LearnedDecisionSource::StrictNoop,
                    fallback_reason: Some(reason.as_fallback_reason()),
                },
                LearnedInferenceMode::Guarded => LearnedDecisionTrace {
                    action: prior.action,
                    prior_action: prior.action,
                    decision_source: LearnedDecisionSource::FallbackRule,
                    fallback_reason: Some(reason.as_fallback_reason()),
                },
            },
        }
    }

    pub fn predict_action(&self, snapshot: &GameSnapshot) -> Option<LearnedPolicyAction> {
        let prior = self.prior_for_snapshot(snapshot);
        self.runtime.predict(snapshot, &prior).ok()
    }

    fn summary_with_trace(
        &self,
        summary: impl Into<String>,
        trace: &LearnedDecisionTrace,
    ) -> String {
        let summary = summary.into();
        if let Some(reason) = trace.fallback_reason {
            format!(
                "{summary} [{}:{}]",
                trace.decision_source.as_str(),
                reason.as_str()
            )
        } else {
            format!("{summary} [{}]", trace.decision_source.as_str())
        }
    }

    fn noop_reason_with_trace(&self, default_reason: &str, trace: &LearnedDecisionTrace) -> String {
        match trace.fallback_reason {
            Some(reason) => format!(
                "{}:{}:{}",
                trace.decision_source.as_str(),
                reason.as_str(),
                default_reason
            ),
            None => format!("{}:{}", trace.decision_source.as_str(), default_reason),
        }
    }
}

impl PhaseStrategy for LearnedKernel {
    fn decide_lobby(&self, snapshot: &GameSnapshot) -> ActionPlan {
        if snapshot.flags.ready_check_active {
            ActionPlan {
                kernel_id: self.kernel_id.clone(),
                summary: format!("Model {} accepted queue", self.model.version),
                confidence: 0.99,
                actions: vec![GameAction::QueueAccept],
            }
        } else {
            ActionPlan::noop(self.kernel_id.clone(), "model waiting in lobby")
        }
    }

    fn decide_augment(&self, snapshot: &GameSnapshot) -> ActionPlan {
        let trace = self.decision_trace(snapshot);
        let action = trace.action;
        if !matches!(action, LearnedPolicyAction::ChooseAugment) || snapshot.augments.is_empty() {
            return ActionPlan::noop(
                self.kernel_id.clone(),
                self.noop_reason_with_trace("model waiting for augment options", &trace),
            );
        }

        let index = self.preferred_augment_index(snapshot);

        ActionPlan {
            kernel_id: self.kernel_id.clone(),
            summary: self.summary_with_trace("Model-selected augment", &trace),
            confidence: 0.86,
            actions: vec![GameAction::ChooseAugment { index }],
        }
    }

    fn decide_shop_economy(&self, snapshot: &GameSnapshot) -> ActionPlan {
        let trace = self.decision_trace(snapshot);
        let action = trace.action;

        match action {
            LearnedPolicyAction::BuyXp if snapshot.flags.can_level && snapshot.gold >= 4 => {
                ActionPlan {
                    kernel_id: self.kernel_id.clone(),
                    summary: self.summary_with_trace(
                        format!("Model {} buys xp", self.model.version),
                        &trace,
                    ),
                    confidence: 0.86,
                    actions: vec![GameAction::BuyXp],
                }
            }
            LearnedPolicyAction::BuyUnit => {
                if let Some(slot) = self.best_slot_by_model(snapshot) {
                    ActionPlan {
                        kernel_id: self.kernel_id.clone(),
                        summary: self.summary_with_trace("Model buys best shop slot", &trace),
                        confidence: 0.84,
                        actions: vec![GameAction::BuyUnit { slot }],
                    }
                } else if snapshot.flags.can_reroll && snapshot.gold >= 2 {
                    ActionPlan {
                        kernel_id: self.kernel_id.clone(),
                        summary: self
                            .summary_with_trace("Model rerolls after empty preferred shop", &trace),
                        confidence: 0.7,
                        actions: vec![GameAction::Reroll],
                    }
                } else {
                    ActionPlan::noop(
                        self.kernel_id.clone(),
                        self.noop_reason_with_trace("model holds without affordable shop", &trace),
                    )
                }
            }
            LearnedPolicyAction::Reroll if snapshot.flags.can_reroll && snapshot.gold >= 2 => {
                ActionPlan {
                    kernel_id: self.kernel_id.clone(),
                    summary: self.summary_with_trace("Model rerolls for upgrade odds", &trace),
                    confidence: 0.74,
                    actions: vec![GameAction::Reroll],
                }
            }
            _ => ActionPlan::noop(
                self.kernel_id.clone(),
                self.noop_reason_with_trace("model preserves econ", &trace),
            ),
        }
    }

    fn decide_board_placement(&self, snapshot: &GameSnapshot) -> ActionPlan {
        let trace = self.decision_trace(snapshot);
        let action = trace.action;
        if !matches!(action, LearnedPolicyAction::MoveBoard) {
            return ActionPlan::noop(
                self.kernel_id.clone(),
                self.noop_reason_with_trace("model preserves current board", &trace),
            );
        }

        if let Some((unit_id, to)) = self.board_move_candidate(snapshot) {
            ActionPlan {
                kernel_id: self.kernel_id.clone(),
                summary: self.summary_with_trace("Model fills board before combat", &trace),
                confidence: 0.83,
                actions: vec![GameAction::MoveBoard { unit_id, to }],
            }
        } else {
            ActionPlan::noop(
                self.kernel_id.clone(),
                self.noop_reason_with_trace("insufficient units for board move", &trace),
            )
        }
    }

    fn decide_itemization(&self, snapshot: &GameSnapshot) -> ActionPlan {
        if let Some(item) = snapshot.items.first() {
            return ActionPlan {
                kernel_id: self.kernel_id.clone(),
                summary: "Model slams tempo item".into(),
                confidence: 0.72,
                actions: vec![GameAction::EquipItem {
                    unit_id: "carry-candidate".into(),
                    item_id: item.clone(),
                }],
            };
        }

        ActionPlan::noop(self.kernel_id.clone(), "no components available")
    }

    fn decide_carousel(&self, _snapshot: &GameSnapshot) -> ActionPlan {
        ActionPlan::noop(self.kernel_id.clone(), "carousel search not wired")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchKernel {
    pub kernel_id: String,
    pub depth: u8,
}

impl Default for SearchKernel {
    fn default() -> Self {
        Self {
            kernel_id: "search-policy".into(),
            depth: 2,
        }
    }
}

impl SearchKernel {
    fn preferred_shop_slot(&self, snapshot: &GameSnapshot) -> Option<u8> {
        snapshot
            .shop
            .iter()
            .find(|slot| {
                list_contains_unit_alias(&snapshot.user_preset.desired_units, &slot.unit_name)
            })
            .map(|slot| slot.index)
    }

    fn augment_score(&self, snapshot: &GameSnapshot, index: u8) -> f32 {
        let Some(augment) = snapshot.augments.get(index as usize) else {
            return -10.0;
        };
        snapshot
            .user_preset
            .augment_priority
            .iter()
            .position(|candidate| augment_name_matches(candidate, augment))
            .map(|rank| 8.0 - rank as f32)
            .unwrap_or(4.0)
    }

    fn action_score(&self, snapshot: &GameSnapshot, action: &GameAction) -> f32 {
        let board_gap = snapshot.level.saturating_sub(snapshot.board.len() as u8) as f32;
        match action {
            GameAction::QueueAccept => 10.0,
            GameAction::ChooseAugment { index } => self.augment_score(snapshot, *index),
            GameAction::MoveBoard { unit_id, .. } => {
                if snapshot.bench.iter().any(|unit| unit.id == *unit_id) {
                    9.0 + board_gap
                } else {
                    -10.0
                }
            }
            GameAction::BuyUnit { slot } => {
                let Some(shop_slot) = snapshot
                    .shop
                    .iter()
                    .find(|candidate| candidate.index == *slot)
                else {
                    return -10.0;
                };
                let preferred = list_contains_unit_alias(
                    &snapshot.user_preset.desired_units,
                    &shop_slot.unit_name,
                ) as i32 as f32;
                let board_needs_help =
                    (snapshot.board.len() < snapshot.level as usize) as i32 as f32;
                4.0 + (preferred * 4.0) + board_needs_help + (shop_slot.cost as f32 * 0.2)
            }
            GameAction::EquipItem { unit_id, item_id } => {
                let Some(unit) = snapshot
                    .board
                    .iter()
                    .chain(&snapshot.bench)
                    .find(|unit| unit.id == *unit_id)
                else {
                    return -10.0;
                };
                if unit.items.len() >= 3 {
                    return -10.0;
                }
                let preferred_unit = list_contains_unit_alias(
                    &snapshot.user_preset.desired_units,
                    &unit.name,
                ) as i32 as f32;
                let preferred_item =
                    list_contains_fuzzy(&snapshot.user_preset.item_priority, &item_id.value) as i32
                        as f32;
                let empty_bonus = (unit.items.is_empty() as i32 as f32) * 0.5;
                3.0 + (preferred_unit * 2.5) + (preferred_item * 2.0) + empty_bonus
            }
            GameAction::SellUnit { unit_id } => {
                let Some(unit) = snapshot.bench.iter().find(|unit| unit.id == *unit_id) else {
                    return -10.0;
                };
                let undesired =
                    (!list_contains_unit_alias(&snapshot.user_preset.desired_units, &unit.name))
                        as i32 as f32;
                let bench_pressure = snapshot.flags.bench_full as i32 as f32;
                let low_value = (unit.stars == 1) as i32 as f32;
                let high_cost_penalty = (unit.cost >= 4) as i32 as f32;
                0.5 + (bench_pressure * 4.5) + (undesired * 2.0) + low_value
                    - (high_cost_penalty * 2.5)
            }
            GameAction::BuyXp => {
                if snapshot.gold >= 50 {
                    7.5
                } else if board_gap <= 1.0 && snapshot.gold >= 12 {
                    4.5
                } else {
                    1.0
                }
            }
            GameAction::Reroll => {
                if self.preferred_shop_slot(snapshot).is_none() && snapshot.gold >= 10 {
                    3.5
                } else {
                    0.5
                }
            }
            GameAction::Noop { .. } => {
                if board_gap > 0.0 && !snapshot.bench.is_empty() {
                    -4.0
                } else if !snapshot.items.is_empty()
                    && snapshot
                        .board
                        .iter()
                        .chain(&snapshot.bench)
                        .any(|unit| unit.items.len() < 3)
                {
                    -3.0
                } else if self.preferred_shop_slot(snapshot).is_some() {
                    -2.0
                } else {
                    0.25
                }
            }
            _ => -5.0,
        }
    }

    fn action_summary(&self, action: &GameAction) -> String {
        match action {
            GameAction::QueueAccept => "Search accepts ready check".into(),
            GameAction::BuyUnit { slot } => format!("Search teacher buys shop slot {slot}"),
            GameAction::BuyXp => format!("Search depth {} buys xp", self.depth),
            GameAction::Reroll => format!("Search depth {} rerolls", self.depth),
            GameAction::MoveBoard { unit_id, .. } => {
                format!("Search teacher moves {unit_id} onto board")
            }
            GameAction::ChooseAugment { index } => {
                format!("Search teacher selects augment {index}")
            }
            GameAction::Noop { .. } => "Search teacher holds".into(),
            GameAction::SellUnit { .. } => "Search teacher sells unit".into(),
            GameAction::MoveBench { .. } => "Search teacher reorders bench".into(),
            GameAction::EquipItem { .. } => "Search teacher equips item".into(),
        }
    }

    fn plan_for_snapshot(&self, snapshot: &GameSnapshot) -> ActionPlan {
        let action = legal_actions(snapshot)
            .into_iter()
            .max_by(|left, right| {
                self.action_score(snapshot, left)
                    .total_cmp(&self.action_score(snapshot, right))
            })
            .unwrap_or(GameAction::Noop {
                reason: "no legal action".into(),
            });
        let score = self.action_score(snapshot, &action);

        ActionPlan {
            kernel_id: self.kernel_id.clone(),
            summary: self.action_summary(&action),
            confidence: (0.45 + (score / 20.0)).clamp(0.1, 0.95),
            actions: vec![action],
        }
    }
}

impl PhaseStrategy for SearchKernel {
    fn decide_lobby(&self, snapshot: &GameSnapshot) -> ActionPlan {
        self.plan_for_snapshot(snapshot)
    }

    fn decide_augment(&self, snapshot: &GameSnapshot) -> ActionPlan {
        self.plan_for_snapshot(snapshot)
    }

    fn decide_shop_economy(&self, snapshot: &GameSnapshot) -> ActionPlan {
        self.plan_for_snapshot(snapshot)
    }

    fn decide_board_placement(&self, snapshot: &GameSnapshot) -> ActionPlan {
        self.plan_for_snapshot(snapshot)
    }

    fn decide_itemization(&self, snapshot: &GameSnapshot) -> ActionPlan {
        self.plan_for_snapshot(snapshot)
    }

    fn decide_carousel(&self, snapshot: &GameSnapshot) -> ActionPlan {
        let target = snapshot
            .user_preset
            .desired_units
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".into());
        ActionPlan {
            kernel_id: self.kernel_id.clone(),
            summary: format!("Carousel: safe hold (target {target})"),
            confidence: 0.95,
            actions: vec![GameAction::Noop {
                reason: "carousel_hold".into(),
            }],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantRuleAdvisor {
    pub kernel_id: String,
}

impl Default for AssistantRuleAdvisor {
    fn default() -> Self {
        Self {
            kernel_id: "assistant-advisor".into(),
        }
    }
}

impl AdvisorKernel for AssistantRuleAdvisor {
    fn id(&self) -> &str {
        &self.kernel_id
    }

    fn advise(&self, snapshot: &GameSnapshot) -> AdvicePayload {
        let mut owned_counts = BTreeMap::<&str, usize>::new();
        for unit in snapshot.board.iter().chain(&snapshot.bench) {
            *owned_counts.entry(unit.name.as_str()).or_default() += 1;
        }

        let mut unit_tips = snapshot
            .shop
            .iter()
            .map(|slot| {
                let preferred =
                    list_contains_unit_alias(&snapshot.user_preset.desired_units, &slot.unit_name);
                let duplicates = owned_counts
                    .get(slot.unit_name.as_str())
                    .copied()
                    .unwrap_or(0);
                let priority = if preferred && duplicates >= 2 {
                    1
                } else if preferred {
                    2
                } else {
                    3
                };
                let detail = if preferred && duplicates >= 2 {
                    format!("已持有 {duplicates} 张，继续拿有机会更快提质量。")
                } else if preferred {
                    "当前主线关键拼图，优先级高于无关打工牌。".into()
                } else if slot
                    .traits
                    .iter()
                    .any(|trait_name| snapshot.user_preset.comp_name.contains(trait_name))
                {
                    "能短暂补羁绊，但价值仍低于主线核心卡。".into()
                } else {
                    "偏离当前主线，除非纯粹为了保战力或凑对子，不建议强拿。".into()
                };

                AdviceItem {
                    label: format!("商店关注 {}", slot.unit_name),
                    detail,
                    priority,
                }
            })
            .collect::<Vec<_>>();
        unit_tips.sort_by_key(|item| item.priority);

        if unit_tips.is_empty() {
            unit_tips = snapshot
                .user_preset
                .desired_units
                .iter()
                .take(3)
                .enumerate()
                .map(|(index, unit)| AdviceItem {
                    label: format!("等下一轮找 {}", unit),
                    detail: "当前商店没刷到关键卡，先稳经济和战力，不急着为凑卡乱 D。".into(),
                    priority: (index + 1) as u8,
                })
                .collect();
        }

        let mut economy_tips = Vec::new();
        if snapshot.flags.pending_augment {
            economy_tips.push(AdviceItem {
                label: "先定强化方向".into(),
                detail: if snapshot.augments.is_empty() {
                    "当前是强化节点，这拍优先看清三项收益，再决定是否转线或补质量。".into()
                } else {
                    format!(
                        "已识别强化候选：{}，先围绕这拍的强化决定后续运营。",
                        snapshot.augments.join(" / ")
                    )
                },
                priority: 1,
            });
        }
        if matches!(snapshot.board_phase, BoardPhase::Combat) {
            economy_tips.push(AdviceItem {
                label: "战斗阶段只观测".into(),
                detail: "当前已进入战斗，重点看结果、掉血和装备掉落，不做主动局内操作。".into(),
                priority: 1,
            });
        } else if snapshot.board.len() + snapshot.bench.len() >= snapshot.level as usize
            && snapshot.board.len() < snapshot.level as usize
        {
            economy_tips.push(AdviceItem {
                label: "先补满人口上限".into(),
                detail: "现有单位数量足够，但场上少上人口，优先把战力补齐再谈贪经济。".into(),
                priority: 1,
            });
        } else if snapshot.gold >= 50 {
            economy_tips.push(AdviceItem {
                label: "优先守住 50".into(),
                detail: "已经站上满利息线，除非明显掉档，不要为了小提升轻易断利息。".into(),
                priority: 1,
            });
        } else if snapshot.gold < 10 {
            economy_tips.push(AdviceItem {
                label: "低金币别乱 D".into(),
                detail: "这拍更需要止住无效消费，围绕关键对子和最低限度战力做决策。".into(),
                priority: 1,
            });
        } else {
            economy_tips.push(AdviceItem {
                label: "看血量决定节奏".into(),
                detail: "能稳血就继续攒利息，真掉档时优先升级补位，再考虑小 D。".into(),
                priority: 1,
            });
        }
        if snapshot.flags.bench_full {
            economy_tips.push(AdviceItem {
                label: "备战席已接近满".into(),
                detail: "下一拍拿牌前先确认是否要上场、合成或清理杂牌，避免继续溢出。".into(),
                priority: 2,
            });
        }

        let item_tips = if snapshot.items.is_empty() {
            snapshot
                .user_preset
                .item_priority
                .iter()
                .enumerate()
                .map(|(index, item)| AdviceItem {
                    label: format!("目标成装 {}", item),
                    detail: "当前装备栏未识别到可用散件，先记住主线装备顺序。".into(),
                    priority: (index + 1) as u8,
                })
                .collect()
        } else {
            snapshot
                .user_preset
                .item_priority
                .iter()
                .enumerate()
                .map(|(index, item)| AdviceItem {
                    label: format!("装备路线 {}", item),
                    detail: format!(
                        "当前已观测散件：{}。优先朝 {} 方向合，避免散件长期空转。",
                        snapshot
                            .items
                            .iter()
                            .map(|i| i.value.as_str())
                            .collect::<Vec<_>>()
                            .join(" / "),
                        item
                    ),
                    priority: (index + 1) as u8,
                })
                .collect()
        };

        AdvicePayload {
            kernel_id: self.kernel_id.clone(),
            comp_name: snapshot.user_preset.comp_name.clone(),
            unit_tips,
            economy_tips,
            item_tips,
            auto_accept: snapshot.flags.ready_check_active,
            record_events: vec![
                format!("stage={}", snapshot.stage),
                format!("gold={}", snapshot.gold),
                format!("board_size={}", snapshot.board.len()),
                format!("bench_size={}", snapshot.bench.len()),
                format!("shop={}", snapshot.shop.len()),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tft_domain::ClientPhase;

    #[test]
    fn learned_kernel_produces_non_empty_plan() {
        let kernel = PhaseRouter::new(LearnedKernel::new(ModelMetadata {
            id: "m1".into(),
            version: "0.1.0".into(),
            family: "policy".into(),
            onnx_path: "artifacts/model.onnx".into(),
            training_dataset: "dataset-v1".into(),
        }));
        let plan = kernel.plan(&GameSnapshot::mock(2, 50));
        assert!(!plan.actions.is_empty());
    }

    #[test]
    fn shop_policy_feature_vector_has_expected_width() {
        let features = policy_features(&GameSnapshot::mock(5, 18));
        assert_eq!(features.len(), POLICY_FEATURE_COUNT);
    }

    #[test]
    fn shop_policy_feature_vector_v2_has_expected_width() {
        let features = policy_features_v2(&GameSnapshot::mock(5, 18));
        assert_eq!(features.len(), POLICY_FEATURE_COUNT_V2);
    }

    #[test]
    fn learned_kernel_can_predict_from_exported_onnx_when_available() {
        let (metadata_path, metadata) =
            if Path::new("artifacts/models/model-metadata-v2.json").exists() {
                (
                    Path::new("artifacts/models/model-metadata-v2.json"),
                    ModelMetadata {
                        id: "learned-v2".into(),
                        version: "2.0.0".into(),
                        family: "policy".into(),
                        onnx_path: "artifacts/models/learned-v2.onnx".into(),
                        training_dataset: "artifacts/dataset-summary.json".into(),
                    },
                )
            } else {
                (
                    Path::new("artifacts/models/model-metadata.json"),
                    ModelMetadata {
                        id: "learned-v1".into(),
                        version: "0.2.0".into(),
                        family: "policy".into(),
                        onnx_path: "artifacts/models/learned-v1.onnx".into(),
                        training_dataset: "artifacts/dataset-summary.json".into(),
                    },
                )
            };
        if !Path::new(&metadata.onnx_path).exists() {
            return;
        }
        if !metadata_path.exists() {
            return;
        }

        let Ok(metadata_json) = std::fs::read_to_string(metadata_path) else {
            return;
        };
        let Ok(parsed): Result<serde_json::Value, _> = serde_json::from_str(&metadata_json) else {
            return;
        };
        let feature_count = parsed
            .get("featureNames")
            .and_then(|value| value.as_array())
            .map(|value| value.len())
            .unwrap_or_default();
        let action_count = parsed
            .get("actionLabels")
            .and_then(|value| value.as_array())
            .map(|value| value.len())
            .unwrap_or_default();
        let multi_head = parsed
            .get("modelHeads")
            .and_then(|value| value.as_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("multi"));
        let expected_feature_count = if multi_head {
            POLICY_FEATURE_COUNT_V2
        } else {
            POLICY_FEATURE_COUNT
        };
        if feature_count != expected_feature_count
            || (!multi_head && action_count != POLICY_ACTION_COUNT)
        {
            return;
        }

        let mut snapshot = GameSnapshot::mock(5, 12);
        snapshot.board_phase = BoardPhase::ShopEconomy;
        let kernel = LearnedKernel::new(metadata);
        assert!(kernel.predict_action(&snapshot).is_some());
    }

    #[test]
    fn strict_mode_emits_strict_noop_when_model_is_missing() {
        let kernel = LearnedKernel::new_with_mode(
            ModelMetadata {
                id: "learned-v2".into(),
                version: "2.0.0".into(),
                family: "policy".into(),
                onnx_path: "artifacts/models/definitely-missing.onnx".into(),
                training_dataset: "artifacts/dataset-summary.json".into(),
            },
            LearnedInferenceMode::Strict,
        );
        let mut snapshot = GameSnapshot::mock(5, 12);
        snapshot.board_phase = BoardPhase::ShopEconomy;
        let trace = kernel.decision_trace(&snapshot);
        assert_eq!(trace.decision_source, LearnedDecisionSource::StrictNoop);
        assert_eq!(
            trace.fallback_reason,
            Some(LearnedFallbackReason::ModelMissing)
        );
        assert_eq!(trace.action, LearnedPolicyAction::Hold);
    }

    #[test]
    fn guarded_mode_falls_back_to_prior_when_model_is_missing() {
        let kernel = LearnedKernel::new_with_mode(
            ModelMetadata {
                id: "learned-v2".into(),
                version: "2.0.0".into(),
                family: "policy".into(),
                onnx_path: "artifacts/models/definitely-missing.onnx".into(),
                training_dataset: "artifacts/dataset-summary.json".into(),
            },
            LearnedInferenceMode::Guarded,
        );
        let mut snapshot = GameSnapshot::mock(5, 12);
        snapshot.board_phase = BoardPhase::ShopEconomy;
        let trace = kernel.decision_trace(&snapshot);
        assert_eq!(trace.decision_source, LearnedDecisionSource::FallbackRule);
        assert_eq!(
            trace.fallback_reason,
            Some(LearnedFallbackReason::ModelMissing)
        );
        assert_eq!(trace.action, trace.prior_action);
    }

    #[test]
    fn hold_prediction_is_not_overridden_by_prior_action() {
        let kernel = LearnedKernel::new_with_mode(
            ModelMetadata {
                id: "learned-v2".into(),
                version: "2.0.0".into(),
                family: "policy".into(),
                onnx_path: "artifacts/models/definitely-missing.onnx".into(),
                training_dataset: "artifacts/dataset-summary.json".into(),
            },
            LearnedInferenceMode::Guarded,
        );
        let mut snapshot = GameSnapshot::mock(5, 12);
        snapshot.board_phase = BoardPhase::ShopEconomy;
        let prior = kernel.prior_for_snapshot(&snapshot);
        assert_eq!(prior.action, LearnedPolicyAction::BuyUnit);
        let trace = kernel.trace_for_action(&snapshot, prior, LearnedPolicyAction::Hold);
        assert_eq!(trace.action, LearnedPolicyAction::Hold);
        assert_eq!(
            trace.decision_source,
            LearnedDecisionSource::LearnedResidual
        );
    }

    #[test]
    fn rule_kernel_board_placement_moves_real_bench_unit() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::BoardPlacement;
        snapshot.level = 4;
        snapshot.bench = vec![UnitInstance {
            id: "bench-1".into(),
            name: "Lux".into(),
            cost: 3,
            stars: 1,
            traits: vec![],
            items: vec![],
            position: None,
            kind: tft_domain::UnitKind::Unit,
        }];

        let kernel = RuleKernel::default();
        let plan = kernel.decide_board_placement(&snapshot);
        assert!(plan.actions.iter().any(
            |action| matches!(action, GameAction::MoveBoard { unit_id, .. } if unit_id == "bench-1")
        ));
        assert!(!plan
            .actions
            .iter()
            .any(|action| matches!(action, GameAction::MoveBoard { unit_id, .. } if unit_id == "bench-anchor")));
    }

    #[test]
    fn rule_kernel_board_placement_prefers_desired_unit_priority_over_bench_order() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::BoardPlacement;
        snapshot.level = 4;
        snapshot.user_preset.desired_units = vec!["Lux".into(), "Poppy".into()];
        snapshot.bench = vec![
            UnitInstance {
                id: "bench-1".into(),
                name: "Poppy".into(),
                cost: 1,
                stars: 1,
                traits: vec![],
                items: vec![],
                position: None,
                kind: tft_domain::UnitKind::Unit,
            },
            UnitInstance {
                id: "bench-2".into(),
                name: "Lux".into(),
                cost: 3,
                stars: 1,
                traits: vec![],
                items: vec![],
                position: None,
                kind: tft_domain::UnitKind::Unit,
            },
        ];

        let kernel = RuleKernel::default();
        let plan = kernel.decide_board_placement(&snapshot);
        assert!(plan.actions.iter().any(
            |action| matches!(action, GameAction::MoveBoard { unit_id, .. } if unit_id == "bench-2")
        ));
    }

    #[test]
    fn rule_kernel_itemization_equips_preferred_item_on_preferred_unit() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::ShopEconomy;
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
        snapshot.user_preset.item_priority = vec!["Rod".into()];
        snapshot.user_preset.desired_units = vec!["Lux".into()];
        snapshot.board = vec![UnitInstance {
            id: "board-1".into(),
            name: "Lux".into(),
            cost: 3,
            stars: 1,
            traits: vec![],
            items: vec![],
            position: Some(BoardPosition { row: 0, column: 0 }),
            kind: tft_domain::UnitKind::Unit,
        }];

        let kernel = RuleKernel::default();
        let plan = kernel.decide_itemization(&snapshot);
        assert!(plan.actions.iter().any(|action| matches!(action, GameAction::EquipItem { unit_id, item_id } if unit_id == "board-1" && item_id.value == "Rod")));
    }

    #[test]
    fn rule_kernel_shop_economy_sells_before_buying_when_bench_is_full() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::ShopEconomy;
        snapshot.gold = 4;
        snapshot.level = 4;
        snapshot.flags.bench_full = true;
        snapshot.user_preset.desired_units = vec!["Lux".into()];
        snapshot.shop = vec![tft_domain::ShopSlot {
            index: 2,
            unit_name: "Lux".into(),
            cost: 3,
            traits: vec![],
        }];
        snapshot.bench = vec![UnitInstance {
            id: "bench-1".into(),
            name: "Poppy".into(),
            cost: 1,
            stars: 1,
            traits: vec![],
            items: vec![],
            position: None,
            kind: tft_domain::UnitKind::Unit,
        }];

        let kernel = RuleKernel::default();
        let plan = kernel.decide_shop_economy(&snapshot);
        assert!(matches!(plan.actions.as_slice(),
            [GameAction::SellUnit { unit_id }, GameAction::BuyUnit { slot }]
            if unit_id == "bench-1" && *slot == 2
        ));
    }

    #[test]
    fn search_kernel_shop_economy_matches_localized_units_against_canonical_shop() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::ShopEconomy;
        snapshot.gold = 4;
        snapshot.level = 4;
        snapshot.user_preset.desired_units = vec!["拉克丝".into()];
        snapshot.shop = vec![tft_domain::ShopSlot {
            index: 2,
            unit_name: "Lux".into(),
            cost: 3,
            traits: vec![],
        }];

        let kernel = SearchKernel::default();
        let plan = kernel.decide_shop_economy(&snapshot);
        assert!(matches!(plan.actions.as_slice(), [GameAction::BuyUnit { slot }] if *slot == 2));
    }

    #[test]
    fn search_kernel_augment_matches_english_priority_against_localized_choices() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::Augment;
        snapshot.user_preset.augment_priority = vec!["Cybernetic Uplink".into()];
        snapshot.augments = vec![
            "组件百宝袋".into(),
            "源计划上行链路 III".into(),
            "珠光莲花 II".into(),
        ];

        let kernel = SearchKernel::default();
        let plan = kernel.decide_augment(&snapshot);
        assert!(
            matches!(plan.actions.as_slice(), [GameAction::ChooseAugment { index }] if *index == 1)
        );
    }

    #[test]
    fn rule_kernel_shop_economy_levels_when_ahead_on_stage_and_units() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::ShopEconomy;
        snapshot.stage = "4-1".into();
        snapshot.gold = 24;
        snapshot.level = 7;
        snapshot.flags.can_level = true;
        snapshot.board = vec![
            UnitInstance {
                id: "board-1".into(),
                name: "Lux".into(),
                cost: 3,
                stars: 1,
                traits: vec![],
                items: vec![],
                position: Some(BoardPosition { row: 0, column: 0 }),
                kind: tft_domain::UnitKind::Unit,
            },
            UnitInstance {
                id: "board-2".into(),
                name: "Poppy".into(),
                cost: 1,
                stars: 1,
                traits: vec![],
                items: vec![],
                position: Some(BoardPosition { row: 0, column: 1 }),
                kind: tft_domain::UnitKind::Unit,
            },
        ];
        snapshot.user_preset.desired_units = vec!["Lux".into(), "Poppy".into()];

        let kernel = RuleKernel::default();
        let plan = kernel.decide_shop_economy(&snapshot);
        assert!(matches!(plan.actions.as_slice(), [GameAction::BuyXp]));
        assert!(plan.summary.contains("level"));
    }

    #[test]
    fn rule_kernel_shop_economy_rerolls_when_no_hit_and_board_is_stable() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::ShopEconomy;
        snapshot.stage = "3-5".into();
        snapshot.gold = 12;
        snapshot.level = 6;
        snapshot.flags.can_reroll = true;
        snapshot.board = vec![
            UnitInstance {
                id: "board-1".into(),
                name: "Lux".into(),
                cost: 3,
                stars: 1,
                traits: vec![],
                items: vec![],
                position: Some(BoardPosition { row: 0, column: 0 }),
                kind: tft_domain::UnitKind::Unit,
            },
            UnitInstance {
                id: "board-2".into(),
                name: "Poppy".into(),
                cost: 1,
                stars: 1,
                traits: vec![],
                items: vec![],
                position: Some(BoardPosition { row: 0, column: 1 }),
                kind: tft_domain::UnitKind::Unit,
            },
        ];
        snapshot.user_preset.desired_units = vec!["Aatrox".into()];

        let kernel = RuleKernel::default();
        let plan = kernel.decide_shop_economy(&snapshot);
        assert!(matches!(plan.actions.as_slice(), [GameAction::Reroll]));
        assert!(plan.summary.contains("preferred board upgrade"));
    }

    #[test]
    fn phase_router_considers_itemization_in_shop_when_primary_holds() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::ShopEconomy;
        snapshot.gold = 0;
        snapshot.flags.can_level = false;
        snapshot.flags.can_reroll = false;
        snapshot.items = vec![TemplateMatchReadout {
            slot: "SLOT_1".into(),
            value: "Bow".into(),
            score: 800,
        }];
        snapshot.bench = vec![UnitInstance {
            id: "bench-1".into(),
            name: "Poppy".into(),
            cost: 1,
            stars: 1,
            traits: vec![],
            items: vec![],
            position: None,
            kind: tft_domain::UnitKind::Unit,
        }];

        let kernel = PhaseRouter::new(RuleKernel::default());
        let plan = kernel.plan(&snapshot);
        assert!(plan
            .actions
            .iter()
            .any(|action| matches!(action, GameAction::EquipItem { .. })));
    }

    #[test]
    fn rule_kernel_carousel_returns_explicit_hold() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::Carousel;
        snapshot.user_preset.desired_units = vec!["Lux".into(), "Poppy".into()];

        let kernel = RuleKernel::default();
        let plan = kernel.decide_carousel(&snapshot);

        assert_eq!(plan.kernel_id, kernel.kernel_id);
        assert!(plan.summary.contains("Carousel"));
        assert!(plan.summary.contains("Lux"));
        assert!(plan.actions.iter().any(|action| matches!(
            action,
            GameAction::Noop { reason } if reason == "carousel_hold"
        )));
    }

    #[test]
    fn search_kernel_carousel_returns_explicit_hold() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.client_phase = ClientPhase::InGame;
        snapshot.board_phase = BoardPhase::Carousel;
        snapshot.user_preset.desired_units = vec!["Lux".into()];

        let kernel = SearchKernel::default();
        let plan = kernel.decide_carousel(&snapshot);

        assert_eq!(plan.kernel_id, kernel.kernel_id);
        assert!(plan.summary.contains("Carousel"));
        assert!(plan.actions.iter().all(|action| matches!(
            action,
            GameAction::Noop { reason } if reason == "carousel_hold"
        )));
    }
}
