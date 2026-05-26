use serde::{Deserialize, Serialize};

/// Discrete macro actions for RL (bounded action space)
///
/// 35 actions covering all game mechanics: shop, economy, board, augment,
/// item, emergency, and positioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum DiscreteAction {
    // === Shop actions (0-6) ===
    Noop = 0,
    BuySlot0 = 1,
    BuySlot1 = 2,
    BuySlot2 = 3,
    BuySlot3 = 4,
    BuySlot4 = 5,
    Reroll = 6,

    // === Economy actions (7-8) ===
    BuyXp = 7,
    LevelUp = 8,

    // === Board management (9-14) ===
    PromoteBestBench = 9,    // Move best bench unit to first open board slot
    FillBoard = 10,          // Fill all open board slots from bench
    SellWeakest = 11,        // Sell weakest bench unit
    SellWeakestBoard = 12,   // Sell weakest board unit
    MoveFrontline = 13,      // Move last-placed board unit to front row
    MoveBackline = 14,       // Move last-placed board unit to back row

    // === Augment actions (15-17) ===
    ChooseAugment0 = 15,
    ChooseAugment1 = 16,
    ChooseAugment2 = 17,

    // === Item actions (18-22) ===
    EquipItemBest = 18,      // Equip first item on best board unit
    EquipItemCarry = 19,     // Equip first item on highest-cost board unit
    EquipItemTank = 20,      // Equip first item on frontline unit
    CombineItems = 21,       // Move items to combine (if recipe match)
    RemoveItems = 22,        // Remove items from weakest unit

    // === Economy advanced (23-25) ===
    HoldGold = 23,           // Intentionally save gold (no spend)
    SpendAll = 24,           // Spend all gold on XP
    InterestReroll = 25,     // Reroll only if above interest threshold (50g)

    // === Board positioning (26-31) ===
    SwapFrontBack = 26,      // Swap frontline and backline units
    MoveLeftFlank = 27,      // Move unit to left column
    MoveRightFlank = 28,     // Move unit to right column
    PromotePair = 29,        // Move bench unit that pairs with board unit
    SellBenchClear = 30,     // Sell all non-preferred bench units
    BoardToBench = 31,       // Move weakest board unit back to bench

    // === Special (32-34) ===
    ChooseAugmentPreferred = 32, // Pick augment matching preset priority
    EmergencySell = 33,      // Sell most expensive unit (gold recovery)
    Scout = 34,              // Noop but with scouting intent (future: read opponents)

    // === Sentinel ===
    ActionCount = 35,
}

impl DiscreteAction {
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Noop),
            1 => Some(Self::BuySlot0),
            2 => Some(Self::BuySlot1),
            3 => Some(Self::BuySlot2),
            4 => Some(Self::BuySlot3),
            5 => Some(Self::BuySlot4),
            6 => Some(Self::Reroll),
            7 => Some(Self::BuyXp),
            8 => Some(Self::LevelUp),
            9 => Some(Self::PromoteBestBench),
            10 => Some(Self::FillBoard),
            11 => Some(Self::SellWeakest),
            12 => Some(Self::SellWeakestBoard),
            13 => Some(Self::MoveFrontline),
            14 => Some(Self::MoveBackline),
            15 => Some(Self::ChooseAugment0),
            16 => Some(Self::ChooseAugment1),
            17 => Some(Self::ChooseAugment2),
            18 => Some(Self::EquipItemBest),
            19 => Some(Self::EquipItemCarry),
            20 => Some(Self::EquipItemTank),
            21 => Some(Self::CombineItems),
            22 => Some(Self::RemoveItems),
            23 => Some(Self::HoldGold),
            24 => Some(Self::SpendAll),
            25 => Some(Self::InterestReroll),
            26 => Some(Self::SwapFrontBack),
            27 => Some(Self::MoveLeftFlank),
            28 => Some(Self::MoveRightFlank),
            29 => Some(Self::PromotePair),
            30 => Some(Self::SellBenchClear),
            31 => Some(Self::BoardToBench),
            32 => Some(Self::ChooseAugmentPreferred),
            33 => Some(Self::EmergencySell),
            34 => Some(Self::Scout),
            _ => None,
        }
    }

    pub fn count() -> usize {
        Self::ActionCount as usize
    }

    pub fn category(&self) -> ActionCategory {
        match self {
            Self::Noop | Self::Scout | Self::HoldGold => ActionCategory::Economy,
            Self::BuySlot0
            | Self::BuySlot1
            | Self::BuySlot2
            | Self::BuySlot3
            | Self::BuySlot4
            | Self::Reroll => ActionCategory::Shop,
            Self::BuyXp | Self::LevelUp | Self::SpendAll | Self::InterestReroll => {
                ActionCategory::Economy
            }
            Self::PromoteBestBench
            | Self::FillBoard
            | Self::SellWeakest
            | Self::SellWeakestBoard
            | Self::MoveFrontline
            | Self::MoveBackline
            | Self::SwapFrontBack
            | Self::MoveLeftFlank
            | Self::MoveRightFlank
            | Self::PromotePair
            | Self::SellBenchClear
            | Self::BoardToBench => ActionCategory::Board,
            Self::ChooseAugment0
            | Self::ChooseAugment1
            | Self::ChooseAugment2
            | Self::ChooseAugmentPreferred => ActionCategory::Augment,
            Self::EquipItemBest
            | Self::EquipItemCarry
            | Self::EquipItemTank
            | Self::CombineItems
            | Self::RemoveItems => ActionCategory::Item,
            Self::EmergencySell => ActionCategory::Emergency,
            Self::ActionCount => ActionCategory::Economy, // sentinel
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionCategory {
    Shop,
    Economy,
    Board,
    Augment,
    Item,
    Emergency,
}

/// Curriculum phase for progressive action-space training.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurriculumPhase {
    /// M0-M1: only shop actions
    ShopOnly,
    /// Early M4: + economy, board basics, augments
    ShopEconomy,
    /// M4+: all actions
    Full,
}

impl CurriculumPhase {
    pub fn allowed_actions(&self) -> Vec<DiscreteAction> {
        match self {
            Self::ShopOnly => vec![
                DiscreteAction::Noop,
                DiscreteAction::BuySlot0,
                DiscreteAction::BuySlot1,
                DiscreteAction::BuySlot2,
                DiscreteAction::BuySlot3,
                DiscreteAction::BuySlot4,
                DiscreteAction::Reroll,
            ],
            Self::ShopEconomy => {
                let mut actions = Self::ShopOnly.allowed_actions();
                actions.extend([
                    DiscreteAction::BuyXp,
                    DiscreteAction::LevelUp,
                    DiscreteAction::SellWeakest,
                    DiscreteAction::PromoteBestBench,
                    DiscreteAction::FillBoard,
                    DiscreteAction::ChooseAugment0,
                    DiscreteAction::ChooseAugment1,
                    DiscreteAction::ChooseAugment2,
                    DiscreteAction::HoldGold,
                ]);
                actions
            }
            Self::Full => (0..DiscreteAction::count() as u16)
                .filter_map(DiscreteAction::from_u16)
                .collect(),
        }
    }
}

/// Curriculum configuration with round-based action unlocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurriculumConfig {
    pub phase: CurriculumPhase,
    pub round_unlocks: Vec<(u8, Vec<DiscreteAction>)>,
}

impl Default for CurriculumConfig {
    fn default() -> Self {
        Self {
            phase: CurriculumPhase::Full,
            round_unlocks: Vec::new(),
        }
    }
}

/// Observation vector for RL agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Obs {
    /// Scalar features: gold, level, xp, health, streak, round, board_count, bench_count
    pub scalars: Vec<f32>,
    /// Shop slot costs (5 slots, 0 if empty)
    pub shop_costs: Vec<f32>,
    /// Whether shop slot matches desired preset (5 slots)
    pub shop_preferred: Vec<f32>,
    /// Board unit count by cost tier (1-5)
    pub board_cost_dist: Vec<f32>,
    /// Phase one-hot: [lobby, augment, shop, placement, combat, post_combat, carousel]
    pub phase: Vec<f32>,
    /// Flags: bench_full, can_level, can_reroll, pending_augment
    pub flags: Vec<f32>,
}

impl Obs {
    pub fn to_vec(&self) -> Vec<f32> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.scalars);
        out.extend_from_slice(&self.shop_costs);
        out.extend_from_slice(&self.shop_preferred);
        out.extend_from_slice(&self.board_cost_dist);
        out.extend_from_slice(&self.phase);
        out.extend_from_slice(&self.flags);
        out
    }

    pub fn dim() -> usize {
        8 + 5 + 5 + 5 + 7 + 4 // = 34
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub obs: Obs,
    pub reward: f32,
    pub terminated: bool,
    pub truncated: bool,
    pub info: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeOutcome {
    pub seed: u64,
    pub total_reward: f32,
    pub steps: usize,
    pub placement: f32,
    pub final_hp: u16,
    pub round_survived: u8,
}

pub trait TftEnv {
    fn reset(&mut self, seed: u64) -> Obs;
    fn step(&mut self, action: DiscreteAction) -> StepResult;
    fn action_count(&self) -> usize;
    fn legal_mask(&self) -> Vec<bool>;
    fn obs_dim(&self) -> usize;
    fn is_done(&self) -> bool;
}

pub mod real_env;
pub mod redline;
pub mod sim_env;
