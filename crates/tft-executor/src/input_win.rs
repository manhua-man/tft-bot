//! Real input simulation using Win32 `SendInput`.
//!
//! Supports both keyboard hotkeys and mouse clicks for buying shop slots.
//! Only compiled when `input_sim` feature is enabled.

use anyhow::{Context, Result};
use std::thread;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::input::InputDispatcher;
use crate::window::{scale_rect, shop_slot_regions, GameWindow};

/// Input mode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Use keyboard keys 1-5 for shop slots
    Hotkey,
    /// Click on shop slot center coordinates
    Mouse,
    /// Try hotkey first, fall back to mouse on failure
    Auto,
}

/// Configuration for real input.
#[derive(Debug, Clone)]
pub struct InputConfig {
    pub mode: InputMode,
    pub click_delay_ms: u64,
    pub ensure_foreground: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            mode: InputMode::Hotkey,
            click_delay_ms: 50,
            ensure_foreground: true,
        }
    }
}

/// Real input dispatcher using Win32 API.
pub struct WinInput {
    config: InputConfig,
}

impl WinInput {
    pub fn new(config: InputConfig) -> Self {
        Self { config }
    }

    pub fn with_defaults() -> Self {
        Self::new(InputConfig::default())
    }

    /// Bring the game window to foreground.
    fn activate_window(&self, window: &GameWindow) -> Result<()> {
        if !self.config.ensure_foreground {
            return Ok(());
        }

        unsafe {
            let title_wide: Vec<u16> = window
                .title
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let hwnd = FindWindowW(None, windows::core::PCWSTR(title_wide.as_ptr()));
            match hwnd {
                Ok(h) if !h.is_invalid() => {
                    let _ = SetForegroundWindow(h);
                    thread::sleep(Duration::from_millis(50));
                    Ok(())
                }
                _ => Err(anyhow::anyhow!(
                    "Could not find window '{}' for foreground activation",
                    window.title
                )),
            }
        }
    }

    /// Send a virtual key press and release.
    fn send_key(vk: VIRTUAL_KEY) -> Result<()> {
        unsafe {
            let inputs = [
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: vk,
                            wScan: 0,
                            dwFlags: KEYBD_EVENT_FLAGS(0),
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                },
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: vk,
                            wScan: 0,
                            dwFlags: KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                },
            ];
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent == inputs.len() as u32 {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "SendInput sent {} of {} events",
                    sent,
                    inputs.len()
                ))
            }
        }
    }

    /// Move mouse to absolute position and click.
    fn send_mouse_click(x: i32, y: i32) -> Result<()> {
        unsafe {
            let move_input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: x,
                        dy: y,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };

            let down_input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_LEFTDOWN,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };

            let up_input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_LEFTUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };

            let inputs = [move_input, down_input, up_input];
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent == inputs.len() as u32 {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Mouse click sent {} of {} events",
                    sent,
                    inputs.len()
                ))
            }
        }
    }

    /// Get screen dimensions for absolute mouse coordinates.
    fn screen_size() -> (u32, u32) {
        unsafe {
            (
                GetSystemMetrics(SM_CXSCREEN) as u32,
                GetSystemMetrics(SM_CYSCREEN) as u32,
            )
        }
    }

    /// Map shop slot index to VIRTUAL_KEY (1-5).
    fn slot_vk(slot: u8) -> VIRTUAL_KEY {
        match slot {
            0 => VK_1,
            1 => VK_2,
            2 => VK_3,
            3 => VK_4,
            4 => VK_5,
            _ => VK_1,
        }
    }
}

impl InputDispatcher for WinInput {
    fn buy_slot(&self, window: &GameWindow, slot: u8) -> Result<()> {
        if slot >= 5 {
            anyhow::bail!("Slot must be 0-4");
        }

        self.activate_window(window)?;
        thread::sleep(Duration::from_millis(self.config.click_delay_ms));

        match self.config.mode {
            InputMode::Hotkey => {
                Self::send_key(Self::slot_vk(slot))
                    .with_context(|| format!("sending hotkey for slot {}", slot))?;
            }
            InputMode::Mouse => {
                let regions = shop_slot_regions();
                let region = regions[slot as usize];
                let (x, y, w, h) = scale_rect(window, region);
                let center_x = x + w / 2;
                let center_y = y + h / 2;

                let (screen_w, screen_h) = Self::screen_size();
                let abs_x = (center_x as f32 / screen_w as f32 * 65535.0) as i32;
                let abs_y = (center_y as f32 / screen_h as f32 * 65535.0) as i32;

                Self::send_mouse_click(abs_x, abs_y).with_context(|| {
                    format!("clicking slot {} at ({}, {})", slot, center_x, center_y)
                })?;
            }
            InputMode::Auto => {
                if Self::send_key(Self::slot_vk(slot)).is_err() {
                    let regions = shop_slot_regions();
                    let region = regions[slot as usize];
                    let (x, y, w, h) = scale_rect(window, region);
                    let center_x = x + w / 2;
                    let center_y = y + h / 2;
                    let (screen_w, screen_h) = Self::screen_size();
                    let abs_x = (center_x as f32 / screen_w as f32 * 65535.0) as i32;
                    let abs_y = (center_y as f32 / screen_h as f32 * 65535.0) as i32;
                    Self::send_mouse_click(abs_x, abs_y)?;
                }
            }
        }

        Ok(())
    }

    fn reroll(&self, window: &GameWindow) -> Result<()> {
        self.activate_window(window)?;
        thread::sleep(Duration::from_millis(self.config.click_delay_ms));
        Self::send_key(VK_D).context("sending reroll hotkey (D)")
    }

    fn buy_xp(&self, window: &GameWindow) -> Result<()> {
        self.activate_window(window)?;
        thread::sleep(Duration::from_millis(self.config.click_delay_ms));
        Self::send_key(VK_F).context("sending buy-xp hotkey (F)")
    }

    fn sell_hovered(&self, window: &GameWindow) -> Result<()> {
        self.activate_window(window)?;
        thread::sleep(Duration::from_millis(self.config.click_delay_ms));
        Self::send_key(VK_E).context("sending sell hotkey (E)")
    }

    fn click_augment(&self, window: &GameWindow, slot: u8) -> Result<()> {
        if slot >= 3 {
            anyhow::bail!("Augment slot must be 0-2");
        }

        self.activate_window(window)?;
        thread::sleep(Duration::from_millis(self.config.click_delay_ms));

        // Augment slots are positioned horizontally in the upper-middle area.
        // These are approximate centers for 1024x768 base resolution.
        // Format: (x, y) in base resolution coordinates
        let augment_positions: [(f32, f32); 3] = [
            (256.0, 350.0), // Left augment
            (512.0, 350.0), // Center augment
            (768.0, 350.0), // Right augment
        ];

        let (base_x, base_y) = augment_positions[slot as usize];
        let scale_x = window.width as f32 / 1024.0;
        let scale_y = window.height as f32 / 768.0;
        let click_x = (base_x * scale_x) as i32 + window.left;
        let click_y = (base_y * scale_y) as i32 + window.top;

        let (screen_w, screen_h) = Self::screen_size();
        let abs_x = (click_x as f32 / screen_w as f32 * 65535.0) as i32;
        let abs_y = (click_y as f32 / screen_h as f32 * 65535.0) as i32;

        Self::send_mouse_click(abs_x, abs_y)
            .with_context(|| format!("clicking augment slot {}", slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = InputConfig::default();
        assert_eq!(config.mode, InputMode::Hotkey);
        assert!(config.ensure_foreground);
    }

    #[test]
    fn slot_vk_mapping() {
        assert_eq!(WinInput::slot_vk(0), VK_1);
        assert_eq!(WinInput::slot_vk(4), VK_5);
    }
}
