//! WinRT OCR engine for Windows.
//!
//! Uses `Windows.Media.Ocr.OcrEngine` for text recognition.
//! Only compiled when `ocr_winrt` feature is enabled.

use anyhow::{Context, Result};
use image::RgbaImage;
use std::sync::OnceLock;

use crate::ocr::{OcrEngine, OcrResult};

use windows::core::{Interface, HSTRING};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine as WinRtOcrEngine;
use windows::Storage::Streams::Buffer;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::WinRT::IBufferByteAccess;

static COM_INIT: OnceLock<()> = OnceLock::new();

fn ensure_com_initialized() {
    COM_INIT.get_or_init(|| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
    });
}

/// WinRT OCR engine (Chinese-first for CN client).
pub struct WinRtOcr {
    engine: WinRtOcrEngine,
    min_confidence: f32,
}

impl WinRtOcr {
    pub fn new(min_confidence: f32) -> Result<Self> {
        ensure_com_initialized();
        let engine = Self::create_engine()?;
        Ok(Self {
            engine,
            min_confidence,
        })
    }

    pub fn with_defaults() -> Result<Self> {
        Self::new(0.25)
    }

    fn create_engine() -> Result<WinRtOcrEngine> {
        // Prefer Simplified Chinese for 国服 shop names.
        if let Ok(lang) = Language::CreateLanguage(&HSTRING::from("zh-Hans")) {
            if let Ok(engine) = WinRtOcrEngine::TryCreateFromLanguage(&lang) {
                return Ok(engine);
            }
        }
        if let Ok(lang) = Language::CreateLanguage(&HSTRING::from("zh-CN")) {
            if let Ok(engine) = WinRtOcrEngine::TryCreateFromLanguage(&lang) {
                return Ok(engine);
            }
        }
        WinRtOcrEngine::TryCreateFromUserProfileLanguages().context(
            "WinRT OcrEngine::TryCreateFromUserProfileLanguages failed; install Chinese OCR language pack",
        )
    }

    fn software_bitmap_from_rgba(image: &RgbaImage) -> Result<SoftwareBitmap> {
        let width = image.width() as i32;
        let height = image.height() as i32;
        if width <= 0 || height <= 0 {
            anyhow::bail!("invalid image dimensions");
        }

        let mut bgra = Vec::with_capacity((width * height * 4) as usize);
        for pixel in image.pixels() {
            bgra.push(pixel[2]);
            bgra.push(pixel[1]);
            bgra.push(pixel[0]);
            bgra.push(pixel[3]);
        }

        let byte_len = bgra.len() as u32;
        let buffer = Buffer::Create(byte_len).context("Buffer::Create")?;
        buffer.SetLength(byte_len).context("Buffer::SetLength")?;

        let access: IBufferByteAccess = buffer
            .cast()
            .context("Buffer -> IBufferByteAccess")?;
        let ptr = unsafe { access.Buffer().context("IBufferByteAccess::Buffer")? };
        unsafe {
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), ptr, bgra.len());
        }

        let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            width,
            height,
            BitmapAlphaMode::Premultiplied,
        )
        .context("SoftwareBitmap::CreateCopyWithAlphaFromBuffer")?;

        Ok(bitmap)
    }

    fn parse_winrt_result(result: &windows::Media::Ocr::OcrResult) -> Result<OcrResult> {
        let text = result
            .Text()
            .context("OcrResult::Text")?
            .to_string();
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return Ok(OcrResult {
                text: String::new(),
                confidence: 0.0,
            });
        }
        // WinRT OcrResult does not expose document-level confidence; use heuristic.
        Ok(OcrResult {
            text: trimmed,
            confidence: 0.85,
        })
    }
}

impl OcrEngine for WinRtOcr {
    fn recognize(&self, image: &RgbaImage) -> Result<OcrResult> {
        if image.width() == 0 || image.height() == 0 {
            return Ok(OcrResult {
                text: String::new(),
                confidence: 0.0,
            });
        }

        let bitmap = Self::software_bitmap_from_rgba(image)?;
        let op = self
            .engine
            .RecognizeAsync(&bitmap)
            .context("OcrEngine::RecognizeAsync")?;
        let result = op.get().context("RecognizeAsync::get")?;
        let parsed = Self::parse_winrt_result(&result)?;
        if parsed.confidence < self.min_confidence && !parsed.text.is_empty() {
            // Keep text but reflect low confidence for downstream filters.
            return Ok(OcrResult {
                confidence: parsed.confidence,
                ..parsed
            });
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(windows, feature = "ocr_winrt"))]
    fn winrt_ocr_reads_synthetic_patch() {
        // White background, dark rectangle — not real text; ensures pipeline runs.
        let mut img = RgbaImage::new(120, 40);
        for p in img.pixels_mut() {
            *p = image::Rgba([240, 240, 240, 255]);
        }
        for x in 20..100 {
            for y in 10..30 {
                img.put_pixel(x, y, image::Rgba([20, 20, 20, 255]));
            }
        }
        let ocr = WinRtOcr::with_defaults().expect("engine");
        let result = ocr.recognize(&img).expect("recognize");
        // May be empty on headless CI; should not error.
        let _ = result.text;
    }
}
