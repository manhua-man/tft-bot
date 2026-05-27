//! Unified executor backend factory.

use crate::correction::OcrCorrectionDict;
use crate::ocr::{OcrEngine, StubOcr};
use crate::window::{StubWindowDiscovery, WindowDiscovery};
use crate::input::{InputDispatcher, StubInput};

pub struct ExecutorBackend {
    pub discovery: Box<dyn WindowDiscovery>,
    pub ocr: Box<dyn OcrEngine>,
    pub input: Box<dyn InputDispatcher>,
    pub corrections: OcrCorrectionDict,
    pub is_real: bool,
}

impl ExecutorBackend {
    pub fn build() -> anyhow::Result<Self> {
        Self::build_with_corrections(OcrCorrectionDict::new())
    }

    pub fn build_with_corrections(corrections: OcrCorrectionDict) -> anyhow::Result<Self> {
        let discovery: Box<dyn WindowDiscovery> = Self::build_discovery();
        let ocr: Box<dyn OcrEngine> = Self::build_ocr();
        let input: Box<dyn InputDispatcher> = Self::build_input();
        let is_real = Self::has_window() && Self::has_input();

        if !Self::has_ocr() {
            eprintln!("[backend] Warning: ocr_winrt not available, using StubOcr");
        }

        Ok(Self { discovery, ocr, input, corrections, is_real })
    }

    #[cfg(feature = "win_window")]
    fn has_window() -> bool { true }
    #[cfg(not(feature = "win_window"))]
    fn has_window() -> bool { false }

    #[cfg(feature = "ocr_winrt")]
    fn has_ocr() -> bool { true }
    #[cfg(not(feature = "ocr_winrt"))]
    fn has_ocr() -> bool { false }

    #[cfg(feature = "input_sim")]
    fn has_input() -> bool { true }
    #[cfg(not(feature = "input_sim"))]
    fn has_input() -> bool { false }

    #[cfg(feature = "win_window")]
    fn build_discovery() -> Box<dyn WindowDiscovery> {
        Box::new(crate::win::window_discovery::WinWindowDiscovery::with_defaults())
    }
    #[cfg(not(feature = "win_window"))]
    fn build_discovery() -> Box<dyn WindowDiscovery> {
        Box::new(StubWindowDiscovery)
    }

    #[cfg(feature = "ocr_winrt")]
    fn build_ocr() -> Box<dyn OcrEngine> {
        match crate::ocr_winrt::WinRtOcr::with_defaults() {
            Ok(engine) => Box::new(engine),
            Err(e) => {
                eprintln!("[backend] WinRT OCR init failed: {e}, using stub");
                Box::new(StubOcr)
            }
        }
    }
    #[cfg(not(feature = "ocr_winrt"))]
    fn build_ocr() -> Box<dyn OcrEngine> {
        Box::new(StubOcr)
    }

    #[cfg(feature = "input_sim")]
    fn build_input() -> Box<dyn InputDispatcher> {
        Box::new(crate::input_win::WinInput::with_defaults())
    }
    #[cfg(not(feature = "input_sim"))]
    fn build_input() -> Box<dyn InputDispatcher> {
        Box::new(StubInput)
    }

    pub fn build_stub() -> Self {
        Self {
            discovery: Box::new(StubWindowDiscovery),
            ocr: Box::new(StubOcr),
            input: Box::new(StubInput),
            corrections: OcrCorrectionDict::new(),
            is_real: false,
        }
    }

    pub fn load_corrections() -> OcrCorrectionDict {
        let path = std::path::Path::new("configs/ocr-corrections.json");
        if path.exists() {
            OcrCorrectionDict::load_from_file(path)
        } else {
            OcrCorrectionDict::new()
        }
    }
}
