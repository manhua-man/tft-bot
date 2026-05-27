//! Window validation — aspect ratio checks and title mismatch detection.
//!
//! Used by preflight and action commands to fail-fast when the game window
//! is in an unexpected state.

use crate::window::GameWindow;

/// Validation result for a game window.
#[derive(Debug, Clone)]
pub struct WindowValidation {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Validate a game window before taking actions.
///
/// Checks:
/// - Aspect ratio is close to 4:3 (1024x768) or 16:9 (1920x1080)
/// - Window dimensions are within expected range
/// - Title contains a known game identifier
pub fn validate_window(window: &GameWindow) -> WindowValidation {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check minimum dimensions
    if window.width < 800 || window.height < 600 {
        errors.push(format!(
            "Window too small: {}x{} (minimum 800x600)",
            window.width, window.height
        ));
    }

    // Check aspect ratio (only if dimensions are valid)
    if window.height > 0 {
        let aspect = window.width as f32 / window.height as f32;
        let expected_43 = 4.0 / 3.0; // 1.333
        let expected_169 = 16.0 / 9.0; // 1.778
        let tolerance = 0.05;

        let is_43 = (aspect - expected_43).abs() < tolerance;
        let is_169 = (aspect - expected_169).abs() < tolerance;

        if !is_43 && !is_169 {
            warnings.push(format!(
                "Unexpected aspect ratio: {:.3} (expected ~{:.3} or ~{:.3})",
                aspect, expected_43, expected_169
            ));
        }
    }

    // Check maximum dimensions (sanity)
    if window.width > 7680 || window.height > 4320 {
        warnings.push(format!(
            "Unusually large window: {}x{}",
            window.width, window.height
        ));
    }

    // Check title contains known game identifiers
    let title_lower = window.title.to_lowercase();
    let known_titles = [
        "英雄联盟",
        "league of legends",
        "云顶之弈",
        "teamfight tactics",
        "tft",
    ];
    let has_known_title = known_titles
        .iter()
        .any(|t| title_lower.contains(t));

    if !has_known_title {
        errors.push(format!(
            "Window title '{}' does not match any known game identifier",
            window.title
        ));
    }

    // Check if window is at negative coordinates (off-screen)
    if window.left < -100 || window.top < -100 {
        warnings.push(format!(
            "Window at unusual position: ({}, {})",
            window.left, window.top
        ));
    }

    WindowValidation {
        ok: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Check if the captured image dimensions match the expected window size.
///
/// Returns an error if the capture is significantly different from the window,
/// which could indicate the window moved or was obscured during capture.
pub fn validate_capture(
    window: &GameWindow,
    captured_width: u32,
    captured_height: u32,
) -> Result<(), String> {
    let w_diff = (window.width as i64 - captured_width as i64).unsigned_abs();
    let h_diff = (window.height as i64 - captured_height as i64).unsigned_abs();

    // Allow 2px tolerance for rounding
    if w_diff > 2 || h_diff > 2 {
        return Err(format!(
            "Capture size mismatch: expected {}x{}, got {}x{}",
            window.width, window.height, captured_width, captured_height
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_window(title: &str, w: u32, h: u32) -> GameWindow {
        GameWindow {
            title: title.to_string(),
            left: 0,
            top: 0,
            width: w,
            height: h,
            dpi: 96,
        }
    }

    #[test]
    fn valid_4x3_window() {
        let v = validate_window(&make_window("英雄联盟", 1024, 768));
        assert!(v.ok, "errors: {:?}", v.errors);
    }

    #[test]
    fn valid_16x9_window() {
        let v = validate_window(&make_window("League of Legends", 1920, 1080));
        assert!(v.ok, "errors: {:?}", v.errors);
    }

    #[test]
    fn too_small_is_error() {
        let v = validate_window(&make_window("英雄联盟", 640, 480));
        assert!(!v.ok);
        assert!(v.errors.iter().any(|e| e.contains("too small")));
    }

    #[test]
    fn unknown_title_is_error() {
        let v = validate_window(&make_window("Notepad", 1024, 768));
        assert!(!v.ok);
        assert!(v.errors.iter().any(|e| e.contains("title")));
    }

    #[test]
    fn unusual_aspect_is_warning() {
        let v = validate_window(&make_window("英雄联盟", 1600, 1000));
        assert!(v.ok); // warning, not error
        assert!(!v.warnings.is_empty());
    }

    #[test]
    fn capture_size_match() {
        assert!(validate_capture(&make_window("t", 1024, 768), 1024, 768).is_ok());
        assert!(validate_capture(&make_window("t", 1024, 768), 1023, 767).is_ok());
    }

    #[test]
    fn capture_size_mismatch() {
        assert!(validate_capture(&make_window("t", 1024, 768), 800, 600).is_err());
    }
}
