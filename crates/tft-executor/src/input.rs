use anyhow::Result;

use crate::window::GameWindow;

pub trait InputDispatcher {
    /// Simulate pressing a shop slot key (1-5)
    fn buy_slot(&self, window: &GameWindow, slot: u8) -> Result<()>;
    /// Simulate pressing D (reroll)
    fn reroll(&self, window: &GameWindow) -> Result<()>;
    /// Simulate pressing F (buy XP)
    fn buy_xp(&self, window: &GameWindow) -> Result<()>;
    /// Simulate pressing E (sell unit under cursor)
    fn sell_hovered(&self, window: &GameWindow) -> Result<()>;
    /// Click an augment slot (0-2) by screen coordinates
    fn click_augment(&self, window: &GameWindow, slot: u8) -> Result<()>;
}

/// Blanket impl for Box<dyn InputDispatcher>
impl InputDispatcher for Box<dyn InputDispatcher> {
    fn buy_slot(&self, window: &GameWindow, slot: u8) -> Result<()> {
        (**self).buy_slot(window, slot)
    }
    fn reroll(&self, window: &GameWindow) -> Result<()> {
        (**self).reroll(window)
    }
    fn buy_xp(&self, window: &GameWindow) -> Result<()> {
        (**self).buy_xp(window)
    }
    fn sell_hovered(&self, window: &GameWindow) -> Result<()> {
        (**self).sell_hovered(window)
    }
    fn click_augment(&self, window: &GameWindow, slot: u8) -> Result<()> {
        (**self).click_augment(window, slot)
    }
}

/// Stub that does nothing (for CI/testing)
pub struct StubInput;

impl InputDispatcher for StubInput {
    fn buy_slot(&self, _window: &GameWindow, _slot: u8) -> Result<()> {
        Err(anyhow::anyhow!(
            "Input simulation requires input_sim feature"
        ))
    }
    fn reroll(&self, _window: &GameWindow) -> Result<()> {
        Err(anyhow::anyhow!(
            "Input simulation requires input_sim feature"
        ))
    }
    fn buy_xp(&self, _window: &GameWindow) -> Result<()> {
        Err(anyhow::anyhow!(
            "Input simulation requires input_sim feature"
        ))
    }
    fn sell_hovered(&self, _window: &GameWindow) -> Result<()> {
        Err(anyhow::anyhow!(
            "Input simulation requires input_sim feature"
        ))
    }
    fn click_augment(&self, _window: &GameWindow, _slot: u8) -> Result<()> {
        Err(anyhow::anyhow!(
            "Input simulation requires input_sim feature"
        ))
    }
}

/// Shop slot key mappings (game default: 1-5 keys)
pub fn slot_key_code(slot: u8) -> Option<u8> {
    match slot {
        0 => Some(b'1'),
        1 => Some(b'2'),
        2 => Some(b'3'),
        3 => Some(b'4'),
        4 => Some(b'5'),
        _ => None,
    }
}
