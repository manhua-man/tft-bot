use anyhow::Result;
use image::RgbaImage;

/// OCR result for a single text region
#[derive(Debug, Clone)]
pub struct OcrResult {
    pub text: String,
    pub confidence: f32,
}

pub trait OcrEngine {
    /// Recognize text from an image region
    fn recognize(&self, image: &RgbaImage) -> Result<OcrResult>;
}

/// Stub OCR that returns empty results (for CI/testing)
pub struct StubOcr;

impl OcrEngine for StubOcr {
    fn recognize(&self, _image: &RgbaImage) -> Result<OcrResult> {
        Ok(OcrResult {
            text: String::new(),
            confidence: 0.0,
        })
    }
}
