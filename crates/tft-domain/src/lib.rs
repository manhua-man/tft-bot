use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

/// Result of a template-matching pass for a single UI slot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMatchReadout {
    /// Slot identifier, e.g. "SLOT_1", "SLOT_2", etc.
    pub slot: String,
    /// Matched value (item name, unit name, etc.).
    pub value: String,
    /// Similarity confidence in milli-units (0–1000, where 1000 = perfect match).
    pub score: u16,
}

pub const REFERENCE_GAME_WIDTH: u32 = 1024;
pub const REFERENCE_GAME_HEIGHT: u32 = 768;
pub const MIN_USABLE_GAME_WIDTH: u32 = 900;
pub const MIN_USABLE_GAME_HEIGHT: u32 = 650;

pub fn normalized_patch(patch: &str) -> Option<String> {
    let trimmed = patch.trim();
    if trimmed.is_empty() || trimmed == "--" || trimmed.eq_ignore_ascii_case("unknown") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn parse_stage_round_label(stage: &str) -> Option<u8> {
    stage
        .split('-')
        .nth(1)
        .and_then(|value| value.trim().parse::<u8>().ok())
}

const PATCH_PACK_SOURCE: &str = include_str!("../../../configs/s16-patch-pack.json");
const AUGMENT_REFERENCE_SOURCE: &str =
    include_str!("../../../configs/augment-reference-s16.ts");

static UNIT_ALIAS_CATALOG: OnceLock<Vec<UnitAliasEntry>> = OnceLock::new();
static AUGMENT_ALIAS_CATALOG: OnceLock<Vec<AugmentAliasEntry>> = OnceLock::new();

#[derive(Debug, Clone)]
struct UnitAliasEntry {
    canonical_name: String,
    normalized_canonical: String,
    normalized_localized: String,
}

#[derive(Debug, Clone)]
struct AugmentAliasEntry {
    display_name_base: String,
    normalized_display: String,
    normalized_display_base: String,
    normalized_internal: String,
    normalized_internal_suffix: String,
    normalized_internal_suffix_base: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClientPhase {
    Idle,
    Lobby,
    QueueReadyCheck,
    Loading,
    InGame,
    PostGame,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BoardPhase {
    Lobby,
    Carousel,
    Augment,
    ShopEconomy,
    BoardPlacement,
    Combat,
    PostCombat,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardPosition {
    pub row: u8,
    pub column: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShopSlot {
    pub index: u8,
    pub unit_name: String,
    pub cost: u8,
    pub traits: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnitInstance {
    pub id: String,
    pub name: String,
    pub cost: u8,
    pub stars: u8,
    pub traits: Vec<String>,
    pub items: Vec<String>,
    pub position: Option<BoardPosition>,
    #[serde(default)]
    pub kind: UnitKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    #[default]
    Unit,
    Placeholder,
    Forge,
    NonOperable,
}

impl UnitKind {
    pub fn is_operable(self) -> bool {
        matches!(self, Self::Unit)
    }

    pub fn is_placeholder(self) -> bool {
        matches!(self, Self::Placeholder)
    }

    pub fn is_special(self) -> bool {
        matches!(self, Self::Forge | Self::NonOperable)
    }
}

impl UnitInstance {
    pub fn is_operable_unit(&self) -> bool {
        self.kind.is_operable()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpponentSnapshot {
    pub player_id: String,
    pub visible_board_strength: u16,
    pub last_seen_round: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserPreset {
    pub comp_name: String,
    pub desired_units: Vec<String>,
    pub item_priority: Vec<String>,
    pub augment_priority: Vec<String>,
}

impl Default for UserPreset {
    fn default() -> Self {
        Self {
            comp_name: "Tempo Sentinel".to_string(),
            desired_units: vec!["Aatrox".into(), "Lux".into(), "Poppy".into()],
            item_priority: vec!["Guinsoo".into(), "Guardbreaker".into(), "Sunfire".into()],
            augment_priority: vec![
                normalize_augment_preference_name("Jeweled Lotus"),
                normalize_augment_preference_name("Cybernetic Uplink"),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFlags {
    pub ready_check_active: bool,
    pub bench_full: bool,
    pub can_level: bool,
    pub can_reroll: bool,
    pub pending_augment: bool,
    pub loot_orbs_visible: bool,
    pub forge_anvil_available: bool,
    pub special_reward_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameSnapshot {
    pub client_phase: ClientPhase,
    pub board_phase: BoardPhase,
    pub patch: String,
    pub stage: String,
    pub round: u8,
    pub gold: u16,
    pub level: u8,
    pub xp: u16,
    pub health: u16,
    pub streak: i16,
    pub shop: Vec<ShopSlot>,
    pub bench: Vec<UnitInstance>,
    pub board: Vec<UnitInstance>,
    pub items: Vec<TemplateMatchReadout>,
    pub augments: Vec<String>,
    pub opponents: Vec<OpponentSnapshot>,
    pub user_preset: UserPreset,
    pub flags: SnapshotFlags,
    pub extra: BTreeMap<String, String>,
}

impl GameSnapshot {
    pub fn empty() -> Self {
        Self {
            client_phase: ClientPhase::Idle,
            board_phase: BoardPhase::Lobby,
            patch: "unknown".to_string(),
            stage: "--".to_string(),
            round: 0,
            gold: 0,
            level: 0,
            xp: 0,
            health: 0,
            streak: 0,
            shop: vec![],
            bench: vec![],
            board: vec![],
            items: vec![],
            augments: vec![],
            opponents: vec![],
            user_preset: UserPreset::default(),
            flags: SnapshotFlags::default(),
            extra: BTreeMap::new(),
        }
    }

    pub fn mock(round: u8, gold: u16) -> Self {
        Self {
            client_phase: ClientPhase::InGame,
            board_phase: if round == 1 {
                BoardPhase::ShopEconomy
            } else if round.is_multiple_of(3) {
                BoardPhase::Augment
            } else if round.is_multiple_of(2) {
                BoardPhase::BoardPlacement
            } else {
                BoardPhase::ShopEconomy
            },
            patch: "14.3-test".to_string(),
            stage: format!("2-{}", round),
            round,
            gold,
            level: 4,
            xp: 6,
            health: 100u16.saturating_sub((round as u16) * 2),
            streak: if round > 2 { 2 } else { 0 },
            shop: vec![
                ShopSlot {
                    index: 0,
                    unit_name: "Aatrox".to_string(),
                    cost: 2,
                    traits: vec!["Bruiser".into(), "Sentinel".into()],
                },
                ShopSlot {
                    index: 1,
                    unit_name: "KogMaw".to_string(),
                    cost: 1,
                    traits: vec!["Sniper".into()],
                },
                ShopSlot {
                    index: 2,
                    unit_name: "Lux".to_string(),
                    cost: 3,
                    traits: vec!["Sorcerer".into(), "Sentinel".into()],
                },
            ],
            bench: vec![],
            board: vec![],
            items: vec![
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
                TemplateMatchReadout {
                    slot: "SLOT_3".into(),
                    value: "Belt".into(),
                    score: 700,
                },
            ],
            augments: vec!["Jeweled Lotus".into(), "Best Friends".into()],
            opponents: vec![OpponentSnapshot {
                player_id: "enemy-1".into(),
                visible_board_strength: 18,
                last_seen_round: "2-1".into(),
            }],
            user_preset: UserPreset::default(),
            flags: SnapshotFlags {
                ready_check_active: false,
                bench_full: false,
                can_level: true,
                can_reroll: gold >= 2,
                pending_augment: round.is_multiple_of(3),
                loot_orbs_visible: false,
                forge_anvil_available: false,
                special_reward_pending: false,
            },
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GameAction {
    QueueAccept,
    BuyUnit {
        slot: u8,
    },
    SellUnit {
        unit_id: String,
    },
    Reroll,
    BuyXp,
    MoveBoard {
        unit_id: String,
        to: BoardPosition,
    },
    MoveBench {
        unit_id: String,
        to_slot: u8,
    },
    EquipItem {
        unit_id: String,
        item_id: TemplateMatchReadout,
    },
    ChooseAugment {
        index: u8,
    },
    Noop {
        reason: String,
    },
}

/// Structured reason code for every automated decision.
/// Format: `{category}:{context}:{detail}` — e.g. `level_up:3-1:below_stage_node`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionCode {
    // Level-up decisions
    LevelUpStageNode,
    LevelUpXpAvailable,
    LevelUpInterestBank,
    LevelUpEmergency,

    // Reroll decisions
    RerollPairHunting,
    RerollDesiredUnits,
    RerollBenchFull,

    // Buy decisions
    BuyDesiredUnit,
    BuyFallbackPriority,
    BuyPairOpportunity,

    // Sell decisions
    SellTrashUnit,
    SellRecoverItem,
    SellBenchOverflow,

    // Board placement decisions
    FieldDesiredUnit,
    FieldReposition,

    // Equip decisions
    EquipBestTarget,
    EquipCoreItem,

    // Economy decisions
    EconomyInterestProtect,
    EconomySpendWindow,

    // Augment decisions
    AugmentLineupMatch,
    AugmentFallback,
    LootPickupWindow,
    ForgeBenchClear,

    // Noop / skip
    SkipCombatLocked,
    SkipCarousel,
    SkipNoLegalAction,
    SkipEarlyGame,

    // General
    Fallback,
}

/// Carries the full explainability context for a single decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionReason {
    pub code: DecisionCode,
    /// Human-readable one-liner, e.g. "level_up:3-1:below_stage_node"
    pub reason: String,
    /// Stage when the decision was made, e.g. "3-1"
    #[serde(default)]
    pub stage: Option<String>,
    /// Confidence of this decision (0.0–1.0)
    #[serde(default)]
    pub confidence: f32,
    /// Additional context key-values (e.g. gold=24, pairs=3)
    #[serde(default)]
    pub context: BTreeMap<String, String>,
}

impl DecisionReason {
    pub fn new(code: DecisionCode, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
            stage: None,
            confidence: 1.0,
            context: BTreeMap::new(),
        }
    }

    pub fn with_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = Some(stage.into());
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    /// Compact code:detail string for logging.
    pub fn to_compact(&self) -> String {
        let code_str = serde_json::to_value(&self.code)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let stage_str = self.stage.as_deref().unwrap_or("?");
        format!("{}:{}:{}", code_str, stage_str, self.reason)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionPlan {
    pub kernel_id: String,
    pub summary: String,
    pub confidence: f32,
    pub actions: Vec<GameAction>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStartMode {
    #[default]
    Auto,
    Lobby,
    Attach,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LineupSourceMeta {
    pub source_id: String,
    pub source_name: String,
    pub source_url: String,
    pub patch: String,
    pub fetched_at: String,
    pub fetched_at_epoch_seconds: u64,
    pub last_success_at: Option<String>,
    pub cached: bool,
    pub stale: bool,
    pub ttl_seconds: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LineupUnitPlan {
    pub unit_name: String,
    pub role: String,
    pub suggested_stars: Option<u8>,
    pub item_recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LineupPosition {
    pub unit_name: String,
    pub row: u8,
    pub column: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LineupCatalogEntry {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub tier: String,
    pub patch: String,
    pub win_rate: Option<f32>,
    pub top4_rate: Option<f32>,
    pub average_placement: Option<f32>,
    pub pick_count: Option<u32>,
    pub difficulty: Option<f32>,
    pub level_plan: Option<String>,
    pub tags: Vec<String>,
    pub core_traits: Vec<String>,
    pub augment_recommendations: Vec<String>,
    pub unit_plans: Vec<LineupUnitPlan>,
    pub board_plan: Vec<LineupPosition>,
    pub source: LineupSourceMeta,
    pub updated_at: String,
    pub updated_at_epoch_seconds: u64,
    pub cache_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LineupCatalog {
    pub patch: String,
    pub generated_at: String,
    pub generated_at_epoch_seconds: u64,
    pub cache_path: String,
    pub status: String,
    pub entries: Vec<LineupCatalogEntry>,
    pub sources: Vec<LineupSourceMeta>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionStepResult {
    pub action: GameAction,
    pub status: String,
    pub message: String,
    pub started_at: String,
    pub finished_at: String,
    pub retries: u8,
    pub artifact_paths: Vec<String>,
    #[serde(default)]
    pub failure_category: Option<String>,
    #[serde(default)]
    pub failure_reason_code: Option<String>,
    #[serde(default)]
    pub decision_reason: Option<DecisionReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutomationPreflightCheck {
    pub label: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutomationPreflightSummary {
    pub status: String,
    pub input_ready: bool,
    pub lobby_ready: bool,
    pub reasons: Vec<String>,
    pub checks: Vec<AutomationPreflightCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRunReport {
    pub run_id: String,
    pub shell: String,
    pub status: String,
    pub start_mode: AutomationStartMode,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub current_phase: String,
    pub selected_strategy_id: String,
    #[serde(default)]
    pub run_mode: String,
    #[serde(default)]
    pub intent_source: Option<String>,
    #[serde(default)]
    pub kernel_used: Option<String>,
    #[serde(default)]
    pub goal_reached: Option<bool>,
    #[serde(default)]
    pub selected_lineup_id: Option<String>,
    #[serde(default)]
    pub selected_lineup_name: Option<String>,
    #[serde(default)]
    pub lineup_selection_source: Option<String>,
    #[serde(default)]
    pub lineup_projection_score: Option<i32>,
    #[serde(default)]
    pub phase_goals: Vec<String>,
    #[serde(default)]
    pub kernel_decisions: Vec<String>,
    #[serde(default)]
    pub goal_verification: Vec<String>,
    #[serde(default)]
    pub preflight: Option<AutomationPreflightSummary>,
    pub credential_source: Option<String>,
    pub config_status: String,
    pub window_status: String,
    pub repair_attempts: u16,
    pub quality_verdict: String,
    pub quality_reasons: Vec<String>,
    pub last_plan_summary: Option<String>,
    pub last_error: Option<String>,
    pub blocking_reason: Option<String>,
    pub artifact_paths: Vec<String>,
    pub episode_ids: Vec<String>,
    pub steps: Vec<ExecutionStepResult>,
    #[serde(default)]
    pub action_stats: BTreeMap<String, ActionExecutionStats>,
    #[serde(default)]
    pub failure_category_counts: BTreeMap<String, u32>,
    #[serde(default)]
    pub failure_reason_counts: BTreeMap<String, u32>,
    #[serde(default)]
    pub redline_counts: BTreeMap<String, u32>,
    #[serde(default)]
    pub observation_health_counts: BTreeMap<String, u32>,
    #[serde(default)]
    pub bad_observation_windows: Vec<String>,
    #[serde(default)]
    pub input_diagnostics: Vec<InputDispatchDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionExecutionStats {
    pub attempted: u32,
    pub input_succeeded: u32,
    pub effect_verified: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct InputDispatchDiagnostic {
    pub action: String,
    pub input_kind: String,
    pub label: String,
    pub dispatch_status: String,
    #[serde(default)]
    pub readiness_status: Option<String>,
    #[serde(default)]
    pub target_x: Option<i32>,
    #[serde(default)]
    pub target_y: Option<i32>,
    #[serde(default)]
    pub target_window_id: Option<i64>,
    #[serde(default)]
    pub target_window_left: Option<i32>,
    #[serde(default)]
    pub target_window_top: Option<i32>,
    #[serde(default)]
    pub target_window_width: Option<u32>,
    #[serde(default)]
    pub target_window_height: Option<u32>,
    #[serde(default)]
    pub foreground_window_id: Option<i64>,
    #[serde(default)]
    pub failure_reason: Option<String>,
}

impl ActionPlan {
    pub fn noop(kernel_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kernel_id: kernel_id.into(),
            summary: "No operation".to_string(),
            confidence: 0.1,
            actions: vec![GameAction::Noop {
                reason: reason.into(),
            }],
        }
    }
}

impl UserPreset {
    pub fn from_lineup(entry: &LineupCatalogEntry) -> Self {
        Self {
            comp_name: entry.name.clone(),
            desired_units: entry
                .unit_plans
                .iter()
                .map(|unit| canonicalize_unit_name(&unit.unit_name))
                .collect(),
            item_priority: entry
                .unit_plans
                .iter()
                .flat_map(|unit| unit.item_recommendations.clone())
                .collect(),
            augment_priority: entry
                .augment_recommendations
                .iter()
                .map(|augment| normalize_augment_preference_name(augment))
                .collect(),
        }
    }
}

pub fn canonicalize_unit_name(raw: &str) -> String {
    resolve_unit_alias_entry(raw)
        .map(|entry| entry.canonical_name.clone())
        .unwrap_or_else(|| raw.trim().to_string())
}

pub fn unit_name_matches(left: &str, right: &str) -> bool {
    if normalized_text_matches(left, right) {
        return true;
    }

    let left_query = normalize_lookup_key(left);
    let right_query = normalize_lookup_key(right);
    match (
        resolve_unit_alias_entry(left),
        resolve_unit_alias_entry(right),
    ) {
        (Some(left_entry), Some(right_entry)) => left_entry
            .canonical_name
            .eq_ignore_ascii_case(&right_entry.canonical_name),
        (Some(entry), None) => unit_alias_entry_matches(entry, &right_query),
        (None, Some(entry)) => unit_alias_entry_matches(entry, &left_query),
        (None, None) => false,
    }
}

pub fn normalize_augment_preference_name(raw: &str) -> String {
    resolve_augment_alias_entry(raw)
        .map(|entry| entry.display_name_base.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| strip_augment_rank_suffix(raw))
}

pub fn augment_name_matches(left: &str, right: &str) -> bool {
    if normalized_text_matches(left, right) {
        return true;
    }

    let left_query = normalize_lookup_key(left);
    let right_query = normalize_lookup_key(right);
    match (
        resolve_augment_alias_entry(left),
        resolve_augment_alias_entry(right),
    ) {
        (Some(left_entry), Some(right_entry)) => {
            augment_alias_entry_matches(left_entry, &right_query)
                && augment_alias_entry_matches(right_entry, &left_query)
        }
        (Some(entry), None) => augment_alias_entry_matches(entry, &right_query),
        (None, Some(entry)) => augment_alias_entry_matches(entry, &left_query),
        (None, None) => false,
    }
}

fn normalize_lookup_key(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || is_cjk_unified_ideograph(*ch))
        .collect::<String>()
        .to_lowercase()
}

fn is_cjk_unified_ideograph(ch: char) -> bool {
    matches!(
        ch,
        '\u{4E00}'..='\u{9FFF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{20000}'..='\u{2A6DF}'
            | '\u{2A700}'..='\u{2B73F}'
            | '\u{2B740}'..='\u{2B81F}'
            | '\u{2B820}'..='\u{2CEAF}'
            | '\u{2CEB0}'..='\u{2EBEF}'
    )
}

fn normalized_text_matches(left: &str, right: &str) -> bool {
    let left = normalize_lookup_key(left);
    let right = normalize_lookup_key(right);
    !left.is_empty()
        && !right.is_empty()
        && (left == right
            || fuzzy_substring_match(&left, &right)
            || fuzzy_substring_match(&right, &left))
}

fn fuzzy_substring_match(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() || haystack.is_empty() || !haystack.contains(needle) {
        return false;
    }

    // 仅在查询词长度达到最小阈值时允许模糊 contains，
    // 避免 "vi"、"源" 这类超短关键词造成误匹配。
    needle.chars().count() >= fuzzy_min_len_for_query(needle)
}

fn fuzzy_min_len_for_query(query: &str) -> usize {
    if query.chars().any(is_cjk_unified_ideograph) {
        2
    } else {
        4
    }
}

fn strip_augment_rank_suffix(raw: &str) -> String {
    let trimmed = raw.trim();
    for suffix in [
        " III", " II", " I", " 3", " 2", " 1", "III", "II", "I", "3", "2", "1",
    ] {
        if let Some(prefix) = trimmed.strip_suffix(suffix) {
            let value = prefix.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }

    trimmed.to_string()
}

fn strip_normalized_augment_rank(mut value: String) -> String {
    while value.ends_with(|ch: char| ch.is_ascii_digit()) {
        value.pop();
    }
    for suffix in ["iii", "ii", "i"] {
        if value.ends_with(suffix) && value.len() > suffix.len() {
            value.truncate(value.len() - suffix.len());
            break;
        }
    }
    value
}

fn unit_alias_catalog() -> &'static [UnitAliasEntry] {
    UNIT_ALIAS_CATALOG.get_or_init(|| {
        patch_pack()
            .units
            .iter()
            .filter_map(|unit| {
                let canonical_name = unit.name.trim();
                let localized_name = unit.localized_name.trim();
                if canonical_name.is_empty() || localized_name.is_empty() {
                    return None;
                }

                Some(UnitAliasEntry {
                    canonical_name: canonical_name.to_string(),
                    normalized_canonical: normalize_lookup_key(canonical_name),
                    normalized_localized: normalize_lookup_key(localized_name),
                })
            })
            .collect()
    })
}

fn augment_alias_catalog() -> &'static [AugmentAliasEntry] {
    AUGMENT_ALIAS_CATALOG.get_or_init(|| parse_augment_alias_catalog(AUGMENT_REFERENCE_SOURCE))
}

fn patch_pack() -> &'static PatchPack {
    static PATCH_PACK: OnceLock<PatchPack> = OnceLock::new();
    PATCH_PACK.get_or_init(|| serde_json::from_str(PATCH_PACK_SOURCE).unwrap_or_default())
}

fn parse_augment_alias_catalog(source: &str) -> Vec<AugmentAliasEntry> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut pending_display_name: Option<String> = None;

    for line in source.lines() {
        let trimmed = line.trim();

        if let Some(value) = trimmed.strip_prefix("\"name\": \"") {
            pending_display_name = Some(extract_quoted_value(value));
            continue;
        }

        let Some(value) = trimmed.strip_prefix("\"augments\": \"") else {
            continue;
        };

        let internal_name = extract_quoted_value(value);
        let Some(display_name) = pending_display_name.take() else {
            continue;
        };
        if display_name.is_empty() || internal_name.is_empty() {
            continue;
        }

        let display_name_base = strip_augment_rank_suffix(&display_name);
        let internal_suffix = internal_name.rsplit('_').next().unwrap_or_default();
        let normalized_display = normalize_lookup_key(&display_name);
        let normalized_display_base = normalize_lookup_key(&display_name_base);
        let normalized_internal = normalize_lookup_key(&internal_name);
        let normalized_internal_suffix = normalize_lookup_key(internal_suffix);
        let normalized_internal_suffix_base =
            strip_normalized_augment_rank(normalized_internal_suffix.clone());
        let dedupe_key = format!("{}:{}", normalized_display, normalized_internal_suffix);
        if normalized_display.is_empty()
            || normalized_internal.is_empty()
            || !seen.insert(dedupe_key)
        {
            continue;
        }

        entries.push(AugmentAliasEntry {
            display_name_base,
            normalized_display,
            normalized_display_base,
            normalized_internal,
            normalized_internal_suffix,
            normalized_internal_suffix_base,
        });
    }

    entries
}

fn extract_quoted_value(raw: &str) -> String {
    raw.split('"').next().unwrap_or_default().trim().to_string()
}

fn resolve_unit_alias_entry(query: &str) -> Option<&'static UnitAliasEntry> {
    let normalized_query = normalize_lookup_key(query);
    if normalized_query.is_empty() {
        return None;
    }

    unit_alias_catalog()
        .iter()
        .find(|entry| {
            entry.normalized_canonical == normalized_query
                || entry.normalized_localized == normalized_query
        })
        .or_else(|| {
            unit_alias_catalog()
                .iter()
                .find(|entry| unit_alias_entry_matches(entry, &normalized_query))
        })
}

fn resolve_augment_alias_entry(query: &str) -> Option<&'static AugmentAliasEntry> {
    let normalized_query = normalize_lookup_key(query);
    if normalized_query.is_empty() {
        return None;
    }

    augment_alias_catalog()
        .iter()
        .find(|entry| {
            entry.normalized_display == normalized_query
                || entry.normalized_display_base == normalized_query
                || entry.normalized_internal == normalized_query
                || entry.normalized_internal_suffix == normalized_query
                || entry.normalized_internal_suffix_base == normalized_query
        })
        .or_else(|| {
            augment_alias_catalog()
                .iter()
                .find(|entry| augment_alias_entry_matches(entry, &normalized_query))
        })
}

fn unit_alias_entry_matches(entry: &UnitAliasEntry, normalized_query: &str) -> bool {
    !normalized_query.is_empty()
        && (fuzzy_substring_match(&entry.normalized_canonical, normalized_query)
            || fuzzy_substring_match(normalized_query, &entry.normalized_canonical)
            || fuzzy_substring_match(&entry.normalized_localized, normalized_query)
            || fuzzy_substring_match(normalized_query, &entry.normalized_localized))
}

fn augment_alias_entry_matches(entry: &AugmentAliasEntry, normalized_query: &str) -> bool {
    !normalized_query.is_empty()
        && [
            entry.normalized_display.as_str(),
            entry.normalized_display_base.as_str(),
            entry.normalized_internal.as_str(),
            entry.normalized_internal_suffix.as_str(),
            entry.normalized_internal_suffix_base.as_str(),
        ]
        .iter()
        .filter(|candidate| !candidate.is_empty())
        .any(|candidate| {
            fuzzy_substring_match(candidate, normalized_query)
                || fuzzy_substring_match(normalized_query, candidate)
        })
}

pub fn first_open_board_slot(snapshot: &GameSnapshot) -> Option<BoardPosition> {
    for row in 0..4 {
        for column in 0..7 {
            let occupied = snapshot.board.iter().any(|unit| {
                unit.position
                    .as_ref()
                    .is_some_and(|position| position.row == row && position.column == column)
            });
            if !occupied {
                return Some(BoardPosition { row, column });
            }
        }
    }

    None
}

pub fn legal_actions(snapshot: &GameSnapshot) -> Vec<GameAction> {
    let mut actions = Vec::new();

    match snapshot.board_phase {
        BoardPhase::Lobby => {
            if snapshot.flags.ready_check_active {
                actions.push(GameAction::QueueAccept);
            }
        }
        BoardPhase::Augment => {
            for (index, _) in snapshot.augments.iter().enumerate() {
                actions.push(GameAction::ChooseAugment { index: index as u8 });
            }
        }
        BoardPhase::ShopEconomy => {
            // Selling is always legal, but we only expose bench sells here to keep the action
            // space bounded and avoid accidental board destruction.
            actions.extend(
                snapshot
                    .bench
                    .iter()
                    .filter(|unit| unit.is_operable_unit())
                    .map(|unit| GameAction::SellUnit {
                        unit_id: unit.id.clone(),
                    }),
            );

            if !snapshot.flags.bench_full {
                actions.extend(
                    snapshot
                        .shop
                        .iter()
                        .filter(|slot| slot.cost as u16 <= snapshot.gold)
                        .map(|slot| GameAction::BuyUnit { slot: slot.index }),
                );
            }

            if snapshot.flags.can_level && snapshot.gold >= 4 {
                actions.push(GameAction::BuyXp);
            }
            if snapshot.flags.can_reroll && snapshot.gold >= 2 {
                actions.push(GameAction::Reroll);
            }

            // Itemization can happen during the shop/economy window.
            extend_equip_item_actions(snapshot, &mut actions);
        }
        BoardPhase::BoardPlacement | BoardPhase::Combat | BoardPhase::PostCombat => {
            if snapshot.board.len() < snapshot.level as usize {
                if let Some(target) = first_open_board_slot(snapshot) {
                    actions.extend(
                        snapshot
                            .bench
                            .iter()
                            .filter(|unit| unit.is_operable_unit())
                            .map(|unit| GameAction::MoveBoard {
                                unit_id: unit.id.clone(),
                                to: target.clone(),
                            }),
                    );
                }
            }

            // Avoid equipping during combat (less predictable and typically locked).
            if !matches!(snapshot.board_phase, BoardPhase::Combat) {
                extend_equip_item_actions(snapshot, &mut actions);
            }
        }
        BoardPhase::Carousel => {
            // Carousel is intentionally modeled as non-actionable for now: caller must be safe.
            return vec![GameAction::Noop {
                reason: "carousel_hold".to_string(),
            }];
        }
    }

    actions.push(GameAction::Noop {
        reason: "hold".to_string(),
    });
    actions
}

fn extend_equip_item_actions(snapshot: &GameSnapshot, actions: &mut Vec<GameAction>) {
    const MAX_UNITS: usize = 12;
    const MAX_ITEMS: usize = 6;
    const MAX_ACTIONS: usize = 48;

    if snapshot.items.is_empty() {
        return;
    }

    let mut unit_count = 0usize;
    for unit in snapshot.board.iter().chain(&snapshot.bench) {
        if unit_count >= MAX_UNITS {
            break;
        }
        if !unit.is_operable_unit() {
            continue;
        }
        if unit.items.len() >= 3 {
            continue;
        }

        let mut item_count = 0usize;
        for item_readout in snapshot.items.iter() {
            if item_count >= MAX_ITEMS || actions.len() >= MAX_ACTIONS {
                break;
            }
            let name = item_readout.value.trim();
            if name.is_empty() {
                continue;
            }
            actions.push(GameAction::EquipItem {
                unit_id: unit.id.clone(),
                item_id: item_readout.clone(),
            });
            item_count += 1;
        }

        unit_count += 1;
        if actions.len() >= MAX_ACTIONS {
            break;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AdviceItem {
    pub label: String,
    pub detail: String,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AdvicePayload {
    pub kernel_id: String,
    pub comp_name: String,
    pub unit_tips: Vec<AdviceItem>,
    pub economy_tips: Vec<AdviceItem>,
    pub item_tips: Vec<AdviceItem>,
    pub auto_accept: bool,
    pub record_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelMetadata {
    pub id: String,
    pub version: String,
    pub family: String,
    pub onnx_path: String,
    pub training_dataset: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatchPack {
    #[serde(default)]
    pub patch: String,
    #[serde(default)]
    pub units: Vec<PatchPackUnit>,
    #[serde(default)]
    pub traits: Vec<String>,
    #[serde(default)]
    pub items: Vec<PatchPackItem>,
    #[serde(default, alias = "stage_rules")]
    pub stage_rules: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatchPackSource {
    #[serde(default)]
    pub chess: String,
    #[serde(default)]
    pub equip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum PatchPackStringList {
    One(String),
    Many(Vec<String>),
}

impl Default for PatchPackStringList {
    fn default() -> Self {
        Self::Many(Vec::new())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatchPackUnit {
    pub name: String,
    #[serde(default, alias = "localizedName")]
    pub localized_name: String,
    #[serde(default)]
    pub cost: u8,
    #[serde(default)]
    pub traits: PatchPackStringList,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatchPackItem {
    pub name: String,
    #[serde(default)]
    pub english_name: String,
    #[serde(default, alias = "type")]
    pub item_type: String,
    #[serde(default)]
    pub recipe_ids: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetricThresholds {
    #[serde(alias = "min_placement_delta")]
    pub min_placement_delta: f32,
    #[serde(alias = "min_blunder_improvement_pct")]
    pub min_blunder_improvement_pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkProtocol {
    #[serde(alias = "seeded_sim_seeds")]
    pub seeded_sim_seeds: Vec<u64>,
    #[serde(alias = "replay_scenarios")]
    pub replay_scenarios: Vec<String>,
    pub thresholds: MetricThresholds,
}

pub mod runtime_config;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn mock_snapshot_has_round_specific_flags() {
        let snapshot = GameSnapshot::mock(3, 12);
        assert!(snapshot.flags.pending_augment);
        assert_eq!(snapshot.stage, "2-3");
    }

    #[test]
    fn legal_actions_include_item_equips_when_items_present() {
        let mut snapshot = GameSnapshot::empty();
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
        snapshot.bench = vec![UnitInstance {
            id: "u1".into(),
            name: "Lux".into(),
            cost: 3,
            stars: 1,
            traits: vec![],
            items: vec![],
            position: None,
            kind: UnitKind::Unit,
        }];

        let actions = legal_actions(&snapshot);
        assert!(actions
            .iter()
            .any(|action| matches!(action, GameAction::EquipItem { .. })));
    }

    #[test]
    fn legal_actions_do_not_offer_equips_for_full_item_slots() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.board_phase = BoardPhase::ShopEconomy;
        snapshot.items = vec![TemplateMatchReadout {
            slot: "SLOT_1".into(),
            value: "Bow".into(),
            score: 800,
        }];
        snapshot.bench = vec![UnitInstance {
            id: "u1".into(),
            name: "Lux".into(),
            cost: 3,
            stars: 1,
            traits: vec![],
            items: vec!["a".into(), "b".into(), "c".into()],
            position: None,
            kind: UnitKind::Unit,
        }];

        let actions = legal_actions(&snapshot);
        assert!(!actions
            .iter()
            .any(|action| matches!(action, GameAction::EquipItem { .. })));
    }

    #[test]
    fn legal_actions_expose_explicit_carousel_noop() {
        let mut snapshot = GameSnapshot::empty();
        snapshot.board_phase = BoardPhase::Carousel;
        let actions = legal_actions(&snapshot);
        assert_eq!(actions.len(), 1);
        assert!(actions.iter().all(
            |action| matches!(action, GameAction::Noop { reason } if reason == "carousel_hold")
        ));
    }

    #[test]
    fn s16_patch_pack_deserializes_with_structured_units_and_items() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("configs")
            .join("s16-patch-pack.json");
        let raw = fs::read_to_string(path).unwrap();
        let pack: PatchPack = serde_json::from_str(&raw).unwrap();

        assert!(!pack.patch.is_empty());
        assert!(!pack.units.is_empty());
        // items may be empty in some patch versions
        assert!(pack.stage_rules.contains_key("2-1"));
    }

    #[test]
    fn unit_alias_matching_bridges_localized_and_canonical_names() {
        assert!(unit_name_matches("Aatrox", "亚托克斯"));
        assert_eq!(canonicalize_unit_name("亚托克斯"), "Aatrox");
    }

    #[test]
    fn augment_alias_matching_bridges_english_and_localized_names() {
        assert!(augment_name_matches("Jeweled Lotus", "珠光莲花 II"));
        assert!(augment_name_matches(
            "Cybernetic Uplink",
            "源计划上行链路 III"
        ));
        assert_eq!(
            normalize_augment_preference_name("Cybernetic Uplink"),
            "源计划上行链路"
        );
    }

    #[test]
    fn lineup_preset_normalizes_units_and_augment_preferences() {
        let preset = UserPreset::from_lineup(&LineupCatalogEntry {
            name: "test".into(),
            augment_recommendations: vec!["珠光莲花 II".into(), "源计划上行链路 III".into()],
            unit_plans: vec![LineupUnitPlan {
                unit_name: "亚托克斯".into(),
                ..LineupUnitPlan::default()
            }],
            ..LineupCatalogEntry::default()
        });

        assert_eq!(preset.desired_units, vec!["Aatrox"]);
        assert_eq!(preset.augment_priority, vec!["珠光莲花", "源计划上行链路"]);
    }

    #[test]
    fn default_preset_uses_localized_augment_preferences() {
        let preset = UserPreset::default();
        assert_eq!(preset.augment_priority, vec!["珠光莲花", "源计划上行链路"]);
    }

    #[test]
    fn normalize_lookup_key_keeps_ascii_and_cjk_only() {
        assert_eq!(normalize_lookup_key("Lux 拉克丝🙂あア한！"), "lux拉克丝");
    }

    #[test]
    fn short_fuzzy_tokens_do_not_match_aliases() {
        // 英文短词（<4）不应触发 contains 模糊匹配。
        assert!(!unit_name_matches("vi", "Viego"));

        // 单个中文字符不应触发 contains 模糊匹配，避免大面积误命中。
        assert!(!fuzzy_substring_match("源计划上行链路", "源"));
        assert!(fuzzy_substring_match("源计划上行链路", "源计"));
    }
}
