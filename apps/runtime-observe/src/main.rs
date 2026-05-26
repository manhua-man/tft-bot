/// runtime-observe: Read game state via OCR and output as JSON.
///
/// Usage:
///   runtime-observe              # Single observation
///   runtime-observe --loop 1000  # Continuous observations (ms interval)
///   runtime-observe --snapshot   # Full GameSnapshot attempt

use std::env;
use std::time::Duration;

use tft_executor::correction::OcrCorrectionDict;
use tft_executor::ocr::StubOcr;
use tft_executor::shop::ShopReader;
use tft_executor::window::{StubWindowDiscovery, WindowDiscovery};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    let continuous = args.iter().position(|a| a == "--loop").and_then(|i| {
        args.get(i + 1).and_then(|s| s.parse::<u64>().ok())
    });

    let discovery = StubWindowDiscovery;
    let window = discovery.find_game_window()?;
    let reader = ShopReader::new(StubOcr, OcrCorrectionDict::new());

    match continuous {
        Some(interval_ms) => {
            eprintln!(
                "Observing every {}ms. Press Ctrl+C to stop.",
                interval_ms
            );
            loop {
                let obs = collect_observation(&reader, &window)?;
                println!("{}", serde_json::to_string(&obs)?);
                std::thread::sleep(Duration::from_millis(interval_ms));
            }
        }
        None => {
            let obs = collect_observation(&reader, &window)?;
            println!("{}", serde_json::to_string_pretty(&obs)?);
        }
    }

    Ok(())
}

fn collect_observation(
    reader: &ShopReader<StubOcr>,
    window: &tft_executor::window::GameWindow,
) -> anyhow::Result<serde_json::Value> {
    let slots = reader.read_shop(window).unwrap_or_default();
    let gold = reader.read_gold(window).ok();

    Ok(serde_json::json!({
        "type": "observation",
        "timestamp": chrono_now(),
        "window": {
            "title": window.title,
            "width": window.width,
            "height": window.height,
        },
        "shop": slots.iter().map(|s| {
            serde_json::json!({
                "index": s.index,
                "raw": s.raw_text,
                "corrected": s.corrected_text,
                "confidence": s.confidence,
            })
        }).collect::<Vec<_>>(),
        "gold": gold,
    }))
}

fn chrono_now() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", dur.as_secs(), dur.subsec_millis())
}
