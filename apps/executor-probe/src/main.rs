use std::env;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("preflight") => cmd_preflight(),
        Some("read-shop") => cmd_read_shop(),
        Some("buy") => {
            // Handle --slot N syntax
            if args.get(2).map(|s| s.as_str()) == Some("--slot") {
                let slot = args
                    .get(3)
                    .and_then(|s| s.parse::<u8>().ok())
                    .ok_or_else(|| anyhow::anyhow!("Usage: executor-probe buy --slot N"))?;
                cmd_buy(slot)
            } else {
                let slot = args
                    .get(2)
                    .and_then(|s| s.parse::<u8>().ok())
                    .ok_or_else(|| anyhow::anyhow!("Usage: executor-probe buy --slot N"))?;
                cmd_buy(slot)
            }
        }
        _ => {
            eprintln!("Usage: executor-probe <command>");
            eprintln!("  preflight     - Check window/capture/OCR/input readiness");
            eprintln!("  read-shop     - Read and print current shop slots");
            eprintln!("  buy --slot N  - Buy shop slot N (0-4) with effectVerified");
            Ok(())
        }
    }
}

fn cmd_preflight() -> anyhow::Result<()> {
    use tft_executor::window::StubWindowDiscovery;
    use tft_executor::window::WindowDiscovery;

    println!("=== Preflight Check ===");

    let discovery = StubWindowDiscovery;
    match discovery.find_game_window() {
        Ok(window) => {
            println!(
                "Window: {} ({}x{})",
                window.title, window.width, window.height
            );
            println!("  Position: ({}, {})", window.left, window.top);
        }
        Err(e) => {
            println!("Window: NOT FOUND ({})", e);
        }
    }

    println!("Capture: stub (screenshots crate available)");
    println!("OCR: stub (no engine configured)");
    println!("Input: stub (input_sim feature not enabled)");
    println!("\nBuild with --features ocr_winrt,input_sim for real machine support.");
    Ok(())
}

fn cmd_read_shop() -> anyhow::Result<()> {
    use tft_executor::correction::OcrCorrectionDict;
    use tft_executor::ocr::StubOcr;
    use tft_executor::shop::ShopReader;
    use tft_executor::window::StubWindowDiscovery;
    use tft_executor::window::WindowDiscovery;

    let discovery = StubWindowDiscovery;
    let window = discovery.find_game_window()?;
    let reader = ShopReader::new(StubOcr, OcrCorrectionDict::new());
    let slots = reader.read_shop(&window)?;

    println!("{}", serde_json::to_string_pretty(&slots)?);
    Ok(())
}

fn cmd_buy(slot: u8) -> anyhow::Result<()> {
    if slot >= 5 {
        anyhow::bail!("Slot must be 0-4");
    }

    use tft_executor::correction::OcrCorrectionDict;
    use tft_executor::input::InputDispatcher;
    use tft_executor::input::StubInput;
    use tft_executor::ocr::StubOcr;
    use tft_executor::shop::ShopReader;
    use tft_executor::verify;
    use tft_executor::window::StubWindowDiscovery;
    use tft_executor::window::WindowDiscovery;

    let discovery = StubWindowDiscovery;
    let window = discovery.find_game_window()?;
    let reader = ShopReader::new(StubOcr, OcrCorrectionDict::new());
    let input = StubInput;

    // Before state
    let gold_before = reader.read_gold(&window).ok();
    let shop_before = reader.read_shop(&window)?;

    println!(
        "Before: gold={:?}, shop={:?}",
        gold_before,
        shop_before
            .iter()
            .map(|s| &s.corrected_text)
            .collect::<Vec<_>>()
    );

    // Execute buy
    input.buy_slot(&window, slot)?;
    println!("Sent buy command for slot {}", slot);

    // Verify
    let result = verify::verify_buy_effect(&reader, &window, gold_before, &shop_before, slot)?;
    println!("After: gold={:?}", result.gold_after);
    println!("Effect verified: {}", result.effect_verified);
    println!("  gold_changed: {}", result.gold_changed);
    println!("  slot_changed: {}", result.slot_changed);

    Ok(())
}
