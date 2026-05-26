use anyhow::Result;

use crate::ocr::OcrEngine;
use crate::shop::ShopReader;
use crate::window::GameWindow;
use crate::ShopSlotReadout;

/// Verify that a buy action had an effect by checking gold/shop changes
pub fn verify_buy_effect<E: OcrEngine>(
    reader: &ShopReader<E>,
    window: &GameWindow,
    gold_before: Option<u16>,
    shop_before: &[ShopSlotReadout],
    slot_bought: u8,
) -> Result<VerifyResult> {
    // Wait a short moment for the game to update
    std::thread::sleep(std::time::Duration::from_millis(300));

    let gold_after = reader.read_gold(window).ok();
    let shop_after = reader.read_shop(window)?;

    let gold_changed = match (gold_before, gold_after) {
        (Some(before), Some(after)) => before != after,
        _ => false,
    };

    let slot_changed = if (slot_bought as usize) < shop_before.len()
        && (slot_bought as usize) < shop_after.len()
    {
        shop_before[slot_bought as usize].corrected_text
            != shop_after[slot_bought as usize].corrected_text
    } else {
        false
    };

    let effect_verified = gold_changed || slot_changed;

    Ok(VerifyResult {
        effect_verified,
        gold_changed,
        slot_changed,
        gold_before,
        gold_after,
        shop_before: shop_before.to_vec(),
        shop_after,
    })
}

#[derive(Debug)]
pub struct VerifyResult {
    pub effect_verified: bool,
    pub gold_changed: bool,
    pub slot_changed: bool,
    pub gold_before: Option<u16>,
    pub gold_after: Option<u16>,
    pub shop_before: Vec<ShopSlotReadout>,
    pub shop_after: Vec<ShopSlotReadout>,
}
