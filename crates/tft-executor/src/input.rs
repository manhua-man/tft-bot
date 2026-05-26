use anyhow::Result;

use crate::window::GameWindow;

pub trait InputDispatcher {
    /// Simulate pressing a shop slot key (1-5)
    fn buy_slot(&self, window: &GameWindow, slot: u8) -> Result<()>;
    /// Simulate pressing D (reroll)
    fn reroll(&self, window: &GameWindow) -> Result<()>;
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
