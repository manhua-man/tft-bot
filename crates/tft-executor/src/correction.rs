use std::collections::BTreeMap;
use std::path::Path;

pub struct OcrCorrectionDict {
    corrections: BTreeMap<String, String>,
}

impl OcrCorrectionDict {
    pub fn new() -> Self {
        Self {
            corrections: BTreeMap::new(),
        }
    }

    pub fn load_from_file(path: &Path) -> Self {
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return Self::new(),
        };

        let entries: Vec<CorrectionEntry> = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => return Self::new(),
        };

        let mut corrections = BTreeMap::new();
        for entry in entries {
            for wrong in entry.incorrect {
                let key = normalize_for_lookup(&wrong);
                if !key.is_empty() {
                    corrections.insert(key, entry.correct.clone());
                }
            }
        }

        Self { corrections }
    }

    pub fn add(&mut self, incorrect: &str, correct: &str) {
        let key = normalize_for_lookup(incorrect);
        if !key.is_empty() {
            self.corrections.insert(key, correct.to_string());
        }
    }

    pub fn correct(&self, raw: &str) -> String {
        // Guard: empty/whitespace text should not be corrected —
        // fuzzy matching could turn "" into a known name like "JarvanIV".
        if raw.trim().is_empty() {
            return raw.to_string();
        }

        let key = normalize_for_lookup(raw);
        if let Some(corrected) = self.corrections.get(&key) {
            return corrected.clone();
        }

        for (wrong, right) in &self.corrections {
            if key.contains(wrong.as_str()) || wrong.contains(key.as_str()) {
                return right.clone();
            }
        }

        raw.to_string()
    }

    pub fn correct_shop_names(&self, names: &mut [String]) {
        for name in names.iter_mut() {
            if !name.is_empty() {
                *name = self.correct(name);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.corrections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.corrections.is_empty()
    }
}

impl Default for OcrCorrectionDict {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_for_lookup(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c > '\u{4E00}')
        .collect::<String>()
        .to_lowercase()
}

#[derive(serde::Deserialize)]
struct CorrectionEntry {
    incorrect: Vec<String>,
    correct: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        let mut dict = OcrCorrectionDict::new();
        dict.add("雅典韩", "雅典娜");
        assert_eq!(dict.correct("雅典韩"), "雅典娜");
    }

    #[test]
    fn no_match_returns_original() {
        let dict = OcrCorrectionDict::new();
        assert_eq!(dict.correct("亚索"), "亚索");
    }

    #[test]
    fn fuzzy_contains_match() {
        let mut dict = OcrCorrectionDict::new();
        dict.add("阿卡丽", "Akali");
        assert_eq!(dict.correct("阿卡丽!"), "Akali");
    }

    #[test]
    fn correct_shop_names_mutates_in_place() {
        let mut dict = OcrCorrectionDict::new();
        dict.add("雅典韩", "雅典娜");
        dict.add("亚素", "亚索");

        let mut names = vec!["雅典韩".into(), "亚素".into(), "永恩".into()];
        dict.correct_shop_names(&mut names);
        assert_eq!(names, vec!["雅典娜", "亚索", "永恩"]);
    }

    #[test]
    fn load_from_json_string() {
        let json = r#"[
            {"incorrect": ["雅典韩", "雅典輸"], "correct": "雅典娜"},
            {"incorrect": ["亚素"], "correct": "亚索"}
        ]"#;

        let temp = std::env::temp_dir().join("tft_executor_test_corrections.json");
        std::fs::write(&temp, json).unwrap();

        let dict = OcrCorrectionDict::load_from_file(&temp);
        assert_eq!(dict.correct("雅典韩"), "雅典娜");
        assert_eq!(dict.correct("雅典輸"), "雅典娜");
        assert_eq!(dict.correct("亚素"), "亚索");

        let _ = std::fs::remove_file(&temp);
    }
}
