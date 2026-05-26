use anyhow::{Context, Result};
use image::RgbaImage;
use screenshots::Screen;

use crate::window::GameWindow;

/// Capture the full game window
pub fn capture_window(window: &GameWindow) -> Result<RgbaImage> {
    let screens = Screen::all().context("listing screens")?;
    let point_x = window.left + 10;
    let point_y = window.top + 10;

    for screen in screens {
        let display = screen.display_info;
        let within_x = point_x >= display.x && point_x < display.x + display.width as i32;
        let within_y = point_y >= display.y && point_y < display.y + display.height as i32;
        if !within_x || !within_y {
            continue;
        }
        let image = screen
            .capture_area(
                window.left - display.x,
                window.top - display.y,
                window.width,
                window.height,
            )
            .context("capturing screen area")?;
        let width = image.width();
        let height = image.height();
        let rgba = image.into_raw();
        return RgbaImage::from_raw(width, height, rgba)
            .ok_or_else(|| anyhow::anyhow!("failed to build RGBA image"));
    }
    Err(anyhow::anyhow!("no screen matched game window coordinates"))
}

/// Crop a region from a captured image
pub fn crop_region(image: &RgbaImage, x: u32, y: u32, width: u32, height: u32) -> RgbaImage {
    image::imageops::crop_imm(image, x, y, width, height).to_image()
}
