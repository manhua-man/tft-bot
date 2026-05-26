use anyhow::Result;

#[derive(Debug, Clone)]
pub struct GameWindow {
    pub title: String,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}

pub trait WindowDiscovery {
    fn find_game_window(&self) -> Result<GameWindow>;
}

/// Reference layout at 1024x768 (the base resolution for TFT)
pub const REFERENCE_WIDTH: u32 = 1024;
pub const REFERENCE_HEIGHT: u32 = 768;

/// Shop slot name regions in reference coordinates (relative to game window)
/// Each region is (left, top, right, bottom) in reference 1024x768 space.
pub fn shop_slot_regions() -> [(f32, f32, f32, f32); 5] {
    // These are approximate regions for the 5 shop name text areas
    // Bottom of screen, evenly spaced across the shop bar
    [
        (110.0, 685.0, 260.0, 720.0), // Slot 0
        (275.0, 685.0, 425.0, 720.0), // Slot 1
        (440.0, 685.0, 590.0, 720.0), // Slot 2
        (605.0, 685.0, 755.0, 720.0), // Slot 3
        (770.0, 685.0, 920.0, 720.0), // Slot 4
    ]
}

/// Gold region in reference coordinates
pub fn gold_region() -> (f32, f32, f32, f32) {
    (860.0, 730.0, 950.0, 760.0)
}

/// Scale a reference rect to actual window size
///
/// Returns `(x, y, width, height)` in actual pixel coordinates, where
/// `(x, y)` is the top-left corner and `(width, height)` are the dimensions.
pub fn scale_rect(window: &GameWindow, rect: (f32, f32, f32, f32)) -> (u32, u32, u32, u32) {
    let sx = window.width as f32 / REFERENCE_WIDTH as f32;
    let sy = window.height as f32 / REFERENCE_HEIGHT as f32;
    (
        (rect.0 * sx) as u32,
        (rect.1 * sy) as u32,
        ((rect.2 - rect.0) * sx) as u32,
        ((rect.3 - rect.1) * sy) as u32,
    )
}

/// Stub implementation that returns an error (no real window discovery without windows crate)
pub struct StubWindowDiscovery;

impl WindowDiscovery for StubWindowDiscovery {
    fn find_game_window(&self) -> Result<GameWindow> {
        Err(anyhow::anyhow!(
            "Window discovery requires windows crate feature"
        ))
    }
}
