use anyhow::Result;

use crate::capture::{capture_window, crop_region};
use crate::correction::OcrCorrectionDict;
use crate::ocr::OcrEngine;
use crate::window::{gold_region, round_region, scale_rect, shop_slot_regions, GameWindow};
use crate::ShopSlotReadout;

pub struct ShopReader<E: OcrEngine> {
    ocr: E,
    corrections: OcrCorrectionDict,
}

impl<E: OcrEngine> ShopReader<E> {
    pub fn new(ocr: E, corrections: OcrCorrectionDict) -> Self {
        Self { ocr, corrections }
    }

    /// Read all 5 shop slots from the game window
    pub fn read_shop(&self, window: &GameWindow) -> Result<Vec<ShopSlotReadout>> {
        let frame = capture_window(window)?;
        let regions = shop_slot_regions();
        let mut slots = Vec::with_capacity(5);

        for (i, region) in regions.iter().enumerate() {
            let (x, y, w, h) = scale_rect(window, *region);
            let cropped = crop_region(&frame, x, y, w, h);
            let ocr_result = self.ocr.recognize(&cropped)?;
            // Skip correction for empty or very low confidence reads —
            // prevents empty slots from being "corrected" into known names.
            let corrected = if ocr_result.text.trim().is_empty() || ocr_result.confidence < 0.15 {
                ocr_result.text.clone()
            } else {
                self.corrections.correct(&ocr_result.text)
            };
            slots.push(ShopSlotReadout {
                index: i as u8,
                raw_text: ocr_result.text,
                corrected_text: corrected,
                confidence: ocr_result.confidence,
            });
        }
        Ok(slots)
    }

    /// Read round/stage text from the game window (e.g. "2-1", "3-2", "4-2").
    ///
    /// Returns the raw OCR text from the round region. Used for augment detection.
    /// Returns empty string if OCR fails or region is unreadable.
    pub fn read_round_text(&self, window: &GameWindow) -> String {
        let frame = match capture_window(window) {
            Ok(f) => f,
            Err(_) => return String::new(),
        };
        let region = round_region();
        let (x, y, w, h) = scale_rect(window, region);
        let cropped = crop_region(&frame, x, y, w, h);
        match self.ocr.recognize(&cropped) {
            Ok(result) => result.text.trim().to_string(),
            Err(_) => String::new(),
        }
    }

    /// Read gold value from the game window
    pub fn read_gold(&self, window: &GameWindow) -> Result<u16> {
        let frame = capture_window(window)?;
        let region = gold_region();
        let (x, y, w, h) = scale_rect(window, region);
        let cropped = crop_region(&frame, x, y, w, h);
        let ocr_result = self.ocr.recognize(&cropped)?;
        ocr_result
            .text
            .trim()
            .parse::<u16>()
            .map_err(|e| anyhow::anyhow!("gold parse error: {}", e))
    }
}
