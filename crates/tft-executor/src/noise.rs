//! Noise filter for shop slot readouts.
//!
//! Filters out placeholder, empty, and unrecognized names that should not
//! drive buy decisions. This prevents the agent from acting on garbage OCR.

use crate::ShopSlotReadout;

/// Known noise patterns that indicate a slot is not a real unit name.
/// These come from OCR misreads of empty/occupied slots.
const NOISE_PATTERNS: &[&str] = &[
    "占位",
    "Occupied",
    "occupied",
    "OCCUPIED",
    "空",
    "---",
    "???",
    "N/A",
    "n/a",
    "null",
    "undefined",
];

/// Configuration for the noise filter.
#[derive(Debug, Clone)]
pub struct NoiseFilterConfig {
    /// Minimum text length to be considered valid (after trim)
    pub min_text_len: usize,
    /// Minimum OCR confidence to be considered valid
    pub min_confidence: f32,
    /// Additional noise patterns (user-configurable blacklist)
    pub extra_blacklist: Vec<String>,
}

impl Default for NoiseFilterConfig {
    fn default() -> Self {
        Self {
            min_text_len: 1,
            min_confidence: 0.1,
            extra_blacklist: Vec::new(),
        }
    }
}

/// Check if a shop slot readout is noise (should not drive buy decisions).
///
/// Returns `true` if the slot is empty, occupied, has low confidence,
/// or matches a known noise pattern.
pub fn is_noise_slot(slot: &ShopSlotReadout, config: &NoiseFilterConfig) -> bool {
    let text = slot.corrected_text.trim();

    // Empty or too short (use char count, not byte length, for CJK correctness)
    if text.chars().count() < config.min_text_len {
        return true;
    }

    // Low confidence
    if slot.confidence < config.min_confidence {
        return true;
    }

    // Known noise patterns
    for pattern in NOISE_PATTERNS {
        if text.contains(pattern) {
            return true;
        }
    }

    // User-configured blacklist
    for pattern in &config.extra_blacklist {
        if text.contains(pattern.as_str()) {
            return true;
        }
    }

    false
}

/// Filter a list of shop slots, returning only those that are not noise.
pub fn filter_valid_slots<'a>(
    slots: &'a [ShopSlotReadout],
    config: &NoiseFilterConfig,
) -> Vec<&'a ShopSlotReadout> {
    slots
        .iter()
        .filter(|s| !is_noise_slot(s, config))
        .collect()
}

/// Check if ALL slots are noise (indicating OCR is not working or not in shop).
pub fn all_slots_noise(slots: &[ShopSlotReadout], config: &NoiseFilterConfig) -> bool {
    slots.iter().all(|s| is_noise_slot(s, config))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_slot(index: u8, text: &str, confidence: f32) -> ShopSlotReadout {
        ShopSlotReadout {
            index,
            raw_text: text.to_string(),
            corrected_text: text.to_string(),
            confidence,
        }
    }

    #[test]
    fn empty_text_is_noise() {
        let config = NoiseFilterConfig::default();
        assert!(is_noise_slot(&make_slot(0, "", 0.8), &config));
    }

    #[test]
    fn occupied_is_noise() {
        let config = NoiseFilterConfig::default();
        assert!(is_noise_slot(&make_slot(0, "占位", 0.9), &config));
        assert!(is_noise_slot(&make_slot(0, "Occupied", 0.9), &config));
    }

    #[test]
    fn low_confidence_is_noise() {
        let config = NoiseFilterConfig::default();
        assert!(is_noise_slot(&make_slot(0, "亚索", 0.05), &config));
    }

    #[test]
    fn valid_unit_is_not_noise() {
        let config = NoiseFilterConfig::default();
        assert!(!is_noise_slot(&make_slot(0, "亚索", 0.8), &config));
        assert!(!is_noise_slot(&make_slot(1, "阿卡丽", 0.7), &config));
    }

    #[test]
    fn extra_blacklist_works() {
        let mut config = NoiseFilterConfig::default();
        config.extra_blacklist.push("测试".to_string());
        assert!(is_noise_slot(&make_slot(0, "测试单位", 0.9), &config));
    }

    #[test]
    fn filter_valid_slots_removes_noise() {
        let config = NoiseFilterConfig::default();
        let slots = vec![
            make_slot(0, "亚索", 0.8),
            make_slot(1, "", 0.0),
            make_slot(2, "占位", 0.9),
            make_slot(3, "阿卡丽", 0.7),
            make_slot(4, "永恩", 0.6),
        ];
        let valid = filter_valid_slots(&slots, &config);
        assert_eq!(valid.len(), 3);
        assert_eq!(valid[0].corrected_text, "亚索");
        assert_eq!(valid[1].corrected_text, "阿卡丽");
        assert_eq!(valid[2].corrected_text, "永恩");
    }

    #[test]
    fn all_slots_noise_when_all_garbage() {
        let config = NoiseFilterConfig::default();
        let slots = vec![
            make_slot(0, "", 0.0),
            make_slot(1, "占位", 0.9),
            make_slot(2, "", 0.0),
        ];
        assert!(all_slots_noise(&slots, &config));
    }
}
