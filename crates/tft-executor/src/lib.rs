pub mod backend;
pub mod capture;
pub mod correction;
pub mod input;
pub mod lcu_gate;
pub mod noise;
pub mod ocr;
pub mod phase;
pub mod shop;
pub mod verify;
pub mod window;
pub mod window_validation;

#[cfg(target_os = "windows")]
pub mod win;

#[cfg(feature = "ocr_winrt")]
pub mod ocr_winrt;

#[cfg(feature = "input_sim")]
pub mod input_win;

use serde::{Deserialize, Serialize};

/// Run a preflight check: window discovery + validation + capture.
///
/// Returns Ok(GameWindow) if all checks pass, Err with diagnostic message otherwise.
/// Used by both executor-probe and ingame loops to ensure the game window is ready.
pub fn preflight_check() -> anyhow::Result<window::GameWindow> {
    let discovery = backend::ExecutorBackend::build_discovery_for_preflight();
    let window = discovery.find_game_window()?;

    // Window validation
    let validation = window_validation::validate_window(&window);
    if !validation.ok {
        anyhow::bail!(
            "Window validation failed: {}",
            validation.errors.join("; ")
        );
    }

    // Capture test
    let img = capture::capture_window(&window)?;
    if let Err(e) = window_validation::validate_capture(&window, img.width(), img.height()) {
        anyhow::bail!("Capture validation failed: {e}");
    }

    Ok(window)
}

/// Result of a shop read operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShopReadout {
    pub slots: Vec<ShopSlotReadout>,
    pub window_title: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShopSlotReadout {
    pub index: u8,
    pub raw_text: String,
    pub corrected_text: String,
    pub confidence: f32,
}

/// Result of a buy action with verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuyResult {
    pub slot: u8,
    pub unit_name: String,
    pub success: bool,
    pub effect_verified: bool,
    pub gold_before: Option<u16>,
    pub gold_after: Option<u16>,
    pub bench_before: Option<usize>,
    pub bench_after: Option<usize>,
    pub shop_changed: bool,
    pub error: Option<String>,
    pub timestamp: String,
}

/// Preflight check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightResult {
    pub window_found: bool,
    pub window_title: Option<String>,
    pub window_size: Option<(u32, u32)>,
    pub capture_ok: bool,
    pub ocr_ok: bool,
    pub input_ok: bool,
    pub errors: Vec<String>,
}

impl PreflightResult {
    pub fn all_ok(&self) -> bool {
        self.window_found
            && self.capture_ok
            && self.ocr_ok
            && self.input_ok
            && self.errors.is_empty()
    }
}
