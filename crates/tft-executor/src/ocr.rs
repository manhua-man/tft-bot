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

/// Blanket impl for Box<dyn OcrEngine>
impl OcrEngine for Box<dyn OcrEngine> {
    fn recognize(&self, image: &RgbaImage) -> Result<OcrResult> {
        (**self).recognize(image)
    }
}

/// Blanket impl for &dyn OcrEngine (used by PhaseDetector::update_visual)
impl OcrEngine for &dyn OcrEngine {
    fn recognize(&self, image: &RgbaImage) -> Result<OcrResult> {
        (**self).recognize(image)
    }
}

/// Blanket impl for references to concrete OCR engines (e.g. `ShopReader::new(&winrt, ...)`).
impl<T: OcrEngine> OcrEngine for &T {
    fn recognize(&self, image: &RgbaImage) -> Result<OcrResult> {
        (*self).recognize(image)
    }
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
