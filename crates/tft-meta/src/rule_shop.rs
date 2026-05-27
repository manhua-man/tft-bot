//! Rule-based shop policy — v1 implementation for Phase 2.
//!
//! Decides which shop slot to buy based on simple rules:
//! - `cheapest`: Buy the slot with the lowest cost (first available)
//! - `first_available`: Buy the first non-empty slot
//! - `random`: Buy a random non-empty slot
//!
//! This is used by `run-afk` when no ONNX model is provided,
//! or as a fallback for Phase 2 acceptance testing.

use rand::Rng;
use tft_env::DiscreteAction;

/// Rule-based shop policy.
pub enum RuleShopPolicy {
    /// Buy the cheapest available slot
    Cheapest,
    /// Buy the first non-empty slot
    FirstAvailable,
    /// Buy a random non-empty slot
    Random,
}

impl RuleShopPolicy {
    /// Choose a buy action based on observed shop slots.
    ///
    /// `slot_texts`: The corrected text for each of the 5 shop slots.
    /// `slot_costs`: The cost of each slot (if known, e.g. from gold changes).
    ///
    /// Returns a DiscreteAction (BuySlot0-4 or Noop if nothing to buy).
    pub fn choose_action(
        &self,
        slot_texts: &[String],
        slot_costs: Option<&[u8]>,
    ) -> DiscreteAction {
        // Find non-empty slots
        let available: Vec<usize> = slot_texts
            .iter()
            .enumerate()
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(i, _)| i)
            .collect();

        if available.is_empty() {
            return DiscreteAction::Noop;
        }

        let slot = match self {
            RuleShopPolicy::Cheapest => {
                if let Some(costs) = slot_costs {
                    // Find the cheapest available slot
                    available
                        .iter()
                        .min_by_key(|&&i| costs.get(i).copied().unwrap_or(5))
                        .copied()
                        .unwrap_or(0)
                } else {
                    // No cost info, just pick first
                    available[0]
                }
            }
            RuleShopPolicy::FirstAvailable => available[0],
            RuleShopPolicy::Random => {
                let mut rng = rand::thread_rng();
                available[rng.gen_range(0..available.len())]
            }
        };

        match slot {
            0 => DiscreteAction::BuySlot0,
            1 => DiscreteAction::BuySlot1,
            2 => DiscreteAction::BuySlot2,
            3 => DiscreteAction::BuySlot3,
            4 => DiscreteAction::BuySlot4,
            _ => DiscreteAction::Noop,
        }
    }
}

impl Default for RuleShopPolicy {
    fn default() -> Self {
        Self::Cheapest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cheapest_ignores_empty() {
        let policy = RuleShopPolicy::Cheapest;
        let slots = vec![
            "".into(),
            "亚索".into(),
            "".into(),
            "永恩".into(),
            "".into(),
        ];
        let action = policy.choose_action(&slots, None);
        assert_eq!(action, DiscreteAction::BuySlot1);
    }

    #[test]
    fn cheapest_with_costs() {
        let policy = RuleShopPolicy::Cheapest;
        let slots = vec![
            "亚索".into(),
            "永恩".into(),
            "劫".into(),
            "".into(),
            "".into(),
        ];
        let costs = [2, 3, 1, 0, 0];
        let action = policy.choose_action(&slots, Some(&costs));
        assert_eq!(action, DiscreteAction::BuySlot2); // cost 1
    }

    #[test]
    fn first_available() {
        let policy = RuleShopPolicy::FirstAvailable;
        let slots = vec!["".into(), "".into(), "劫".into(), "永恩".into(), "".into()];
        let action = policy.choose_action(&slots, None);
        assert_eq!(action, DiscreteAction::BuySlot2);
    }

    #[test]
    fn all_empty_returns_noop() {
        let policy = RuleShopPolicy::Cheapest;
        let slots = vec!["".into(), "".into(), "".into(), "".into(), "".into()];
        let action = policy.choose_action(&slots, None);
        assert_eq!(action, DiscreteAction::Noop);
    }
}
