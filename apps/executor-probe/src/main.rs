use std::env;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("preflight") => cmd_preflight(&args[2..]),
        Some("read-shop") => cmd_read_shop(&args[2..]),
        Some("buy") => {
            let slot = parse_slot(&args[2..])?;
            cmd_buy(slot, &args[2..])
        }
        Some("calibrate") => cmd_calibrate(&args[2..]),
        _ => {
            eprintln!("Usage: executor-probe <command> [options]");
            eprintln!();
            eprintln!("Commands:");
            eprintln!("  preflight [--stub]     Check window/capture/OCR/input/LCU readiness");
            eprintln!("  read-shop [--stub]     Read and print current shop slots (with noise filter)");
            eprintln!("  buy --slot N [--stub]  Buy shop slot N (0-4) with effectVerified");
            eprintln!("  calibrate [--stub]     Show calibration info and crop previews");
            eprintln!();
            eprintln!("Options:");
            eprintln!("  --stub         Use stub backends (for testing/CI)");
            eprintln!("  --no-lcu       Skip LCU gate (allow actions without phase check)");
            eprintln!("  --lockfile P   Override LCU lockfile path");
            Ok(())
        }
    }
}

fn has_stub_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "--stub")
}

fn has_no_lcu_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "--no-lcu")
}

fn get_lockfile_path(args: &[String]) -> String {
    for (i, a) in args.iter().enumerate() {
        if a == "--lockfile" {
            if let Some(path) = args.get(i + 1) {
                return path.clone();
            }
        }
    }
    // Env override
    if let Ok(p) = std::env::var("LCU_LOCKFILE") {
        if !p.trim().is_empty() {
            return p;
        }
    }
    tft_executor::lcu_gate::resolve_lockfile_path()
}

fn parse_slot(args: &[String]) -> anyhow::Result<u8> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--slot" {
            i += 1;
            if i < args.len() {
                return args[i]
                    .parse::<u8>()
                    .map_err(|_| anyhow::anyhow!("Invalid slot value: {}", args[i]));
            }
        } else if let Ok(s) = args[i].parse::<u8>() {
            return Ok(s);
        }
        i += 1;
    }
    Err(anyhow::anyhow!("Usage: executor-probe buy --slot N"))
}

// -- Backend factory ----------------------------------------------------------

struct Backend {
    discovery: Option<tft_executor::win::window_discovery::WinWindowDiscovery>,
    input: Option<tft_executor::input_win::WinInput>,
    is_real: bool,
}

impl Backend {
    fn new(use_stub: bool) -> anyhow::Result<Self> {
        if use_stub {
            return Ok(Self { discovery: None, input: None, is_real: false });
        }
        let has_win = cfg!(feature = "win_window");
        let has_input = cfg!(feature = "input_sim");
        let discovery = if has_win {
            Some(tft_executor::win::window_discovery::WinWindowDiscovery::with_defaults())
        } else {
            None
        };
        let input = if has_input {
            Some(tft_executor::input_win::WinInput::with_defaults())
        } else {
            None
        };
        if !has_win { eprintln!("[backend] win_window not available"); }
        if !cfg!(feature = "ocr_winrt") { eprintln!("[backend] ocr_winrt not available, using StubOcr"); }
        if !has_input { eprintln!("[backend] input_sim not available"); }
        Ok(Self { discovery, input, is_real: has_win && has_input })
    }

    fn find_window(&self) -> anyhow::Result<tft_executor::window::GameWindow> {
        use tft_executor::window::WindowDiscovery;
        match &self.discovery {
            Some(d) => d.find_game_window(),
            None => Err(anyhow::anyhow!("Window discovery requires win_window feature")),
        }
    }

    fn read_shop(&self, window: &tft_executor::window::GameWindow) -> anyhow::Result<Vec<tft_executor::ShopSlotReadout>> {
        use tft_executor::correction::OcrCorrectionDict;
        use tft_executor::shop::ShopReader;
        let reader = ShopReader::new(tft_executor::ocr::StubOcr, load_corrections());
        reader.read_shop(window)
    }

    fn read_gold(&self, window: &tft_executor::window::GameWindow) -> anyhow::Result<u16> {
        use tft_executor::correction::OcrCorrectionDict;
        use tft_executor::shop::ShopReader;
        let reader = ShopReader::new(tft_executor::ocr::StubOcr, OcrCorrectionDict::new());
        reader.read_gold(window)
    }

    fn buy_slot(&self, window: &tft_executor::window::GameWindow, slot: u8) -> anyhow::Result<()> {
        use tft_executor::input::InputDispatcher;
        match &self.input {
            Some(i) => i.buy_slot(window, slot),
            None => Err(anyhow::anyhow!("Input simulation requires input_sim feature")),
        }
    }
}

fn load_corrections() -> tft_executor::correction::OcrCorrectionDict {
    let path = std::path::Path::new("configs/ocr-corrections.json");
    if path.exists() {
        tft_executor::correction::OcrCorrectionDict::load_from_file(path)
    } else {
        tft_executor::correction::OcrCorrectionDict::new()
    }
}

// -- Commands -----------------------------------------------------------------

fn cmd_preflight(args: &[String]) -> anyhow::Result<()> {
    use tft_executor::lcu_gate::LcuGate;
    use tft_executor::window_validation::validate_window;

    let use_stub = has_stub_flag(args);
    let backend = Backend::new(use_stub)?;
    let lockfile_path = get_lockfile_path(args);

    println!("=== Preflight Check ===");
    println!("Backend: {}", if use_stub { "stub" } else { "real" });

    // LCU probe
    println!("\n--- LCU ---");
    let lcu_gate = LcuGate::probe(&lockfile_path);
    let probe = lcu_gate.probe_result();
    println!("Lockfile: {}", if probe.available { "OK" } else { "N/A" });
    if let Some(ref lf) = probe.lockfile {
        println!("  port: {}, pid: {}", lf.port, lf.pid);
    }
    if let Some(ref phase) = probe.phase {
        println!("Phase: {}", phase);
        println!("  can_act: {}", phase.can_act());
    }
    if let Some(ref err) = probe.error {
        println!("Error: {}", err);
    }
    println!("Verdict: {}", if probe.available {
        format!("LCU_OK (phase={})", probe.phase.as_ref().unwrap())
    } else {
        "LCU_NOT_AVAILABLE (will use visual fallback)".to_string()
    });

    // Window
    println!("\n--- Window ---");
    match backend.find_window() {
        Ok(window) => {
            println!(
                "Window: {} ({}x{}) at ({},{})",
                window.title, window.width, window.height, window.left, window.top
            );
            println!("  DPI: {}", window.dpi);

            // Window validation
            let validation = validate_window(&window);
            if validation.ok {
                println!("Validation: OK");
            } else {
                println!("Validation: FAIL");
                for err in &validation.errors {
                    println!("  ERROR: {}", err);
                }
            }
            for warn in &validation.warnings {
                println!("  WARNING: {}", warn);
            }

            // Capture test
            match tft_executor::capture::capture_window(&window) {
                Ok(img) => {
                    println!("Capture: OK ({}x{})", img.width(), img.height());

                    // Validate capture dimensions match window
                    if let Err(e) = tft_executor::window_validation::validate_capture(
                        &window,
                        img.width(),
                        img.height(),
                    ) {
                        println!("  WARNING: {}", e);
                    }
                }
                Err(e) => {
                    println!("Capture: FAIL ({})", e);
                }
            }

            // OCR test
            let slots = backend.read_shop(&window).unwrap_or_default();
            let noise_config = tft_executor::noise::NoiseFilterConfig::default();
            let valid_slots = tft_executor::noise::filter_valid_slots(&slots, &noise_config);
            let all_noise = tft_executor::noise::all_slots_noise(&slots, &noise_config);
            println!(
                "OCR: {} valid slots (of 5), all_noise={}",
                valid_slots.len(),
                all_noise
            );

            // Input test
            println!(
                "Input: {}",
                if use_stub { "stub" } else { "available" }
            );
        }
        Err(e) => {
            println!("Window: NOT FOUND ({})", e);
        }
    }

    if !use_stub {
        println!("\nFeatures:");
        println!("  win_window: {}", cfg!(feature = "win_window"));
        println!("  ocr_winrt:  {}", cfg!(feature = "ocr_winrt"));
        println!("  input_sim:  {}", cfg!(feature = "input_sim"));
    }

    Ok(())
}

fn cmd_read_shop(args: &[String]) -> anyhow::Result<()> {
    use tft_executor::noise::{filter_valid_slots, NoiseFilterConfig};

    let use_stub = has_stub_flag(args);
    let backend = Backend::new(use_stub)?;

    let window = backend.find_window()?;
    let slots = backend.read_shop(&window)?;

    // Print all slots
    println!("=== Raw Shop Slots ===");
    println!("{}", serde_json::to_string_pretty(&slots)?);

    // Print noise-filtered slots
    let config = NoiseFilterConfig::default();
    let valid = filter_valid_slots(&slots, &config);
    println!("\n=== Valid Slots (noise filtered) ===");
    for s in &valid {
        println!("  [{}] {} (conf={:.2})", s.index, s.corrected_text, s.confidence);
    }
    if valid.is_empty() {
        println!("  (all slots filtered as noise — not in shop phase?)");
    }

    Ok(())
}

fn cmd_buy(slot: u8, args: &[String]) -> anyhow::Result<()> {
    use tft_executor::lcu_gate::LcuGate;
    use tft_executor::noise::{is_noise_slot, NoiseFilterConfig};

    if slot >= 5 {
        anyhow::bail!("Slot must be 0-4");
    }

    let use_stub = has_stub_flag(args);
    let skip_lcu = has_no_lcu_flag(args);
    let lockfile_path = get_lockfile_path(args);
    let backend = Backend::new(use_stub)?;

    // LCU phase gate
    if !skip_lcu {
        let lcu_gate = LcuGate::probe(&lockfile_path);
        let perm = lcu_gate.check_can_act(false)?;
        if !perm.allowed {
            anyhow::bail!(
                "Action blocked by LCU phase gate: {}",
                perm.reason
            );
        }
        eprintln!("[lcu] {}", perm.reason);
    }

    let window = backend.find_window()?;

    // Before state
    let gold_before = backend.read_gold(&window).ok();
    let shop_before = backend.read_shop(&window)?;

    // Noise check — warn if the target slot is noise
    let noise_config = NoiseFilterConfig::default();
    if let Some(target_slot) = shop_before.get(slot as usize) {
        if is_noise_slot(target_slot, &noise_config) {
            eprintln!(
                "WARNING: Slot {} appears to be noise ('{}', conf={:.2})",
                slot, target_slot.corrected_text, target_slot.confidence
            );
            eprintln!("  Proceeding anyway, but effect_verified may fail.");
        }
    }

    println!(
        "Before: gold={:?}, shop={:?}",
        gold_before,
        shop_before
            .iter()
            .map(|s| &s.corrected_text)
            .collect::<Vec<_>>()
    );

    // Execute buy
    backend.buy_slot(&window, slot)?;
    println!("Sent buy command for slot {}", slot);

    // Verify
    use tft_executor::verify;
    {
        let reader = tft_executor::shop::ShopReader::new(tft_executor::ocr::StubOcr, load_corrections());
        let result = verify::verify_buy_effect(&reader, &window, gold_before, &shop_before, slot)?;
        println!("After: gold={:?}", result.gold_after);
        println!("Effect verified: {}", result.effect_verified);
        println!("  gold_changed: {}", result.gold_changed);
        println!("  slot_changed: {}", result.slot_changed);
    }

    Ok(())
}

fn cmd_calibrate(args: &[String]) -> anyhow::Result<()> {
    let use_stub = has_stub_flag(args);
    let backend = Backend::new(use_stub)?;

    println!("=== Calibration ===");

    let window = backend.find_window()?;
    println!(
        "Window: {} ({}x{})",
        window.title, window.width, window.height
    );

    // Show reference regions scaled to actual window
    let regions = tft_executor::window::shop_slot_regions();
    for (i, region) in regions.iter().enumerate() {
        let (x, y, w, h) = tft_executor::window::scale_rect(&window, *region);
        println!("  Slot {}: ({}, {}) {}x{}", i, x, y, w, h);
    }

    let gold = tft_executor::window::gold_region();
    let (gx, gy, gw, gh) = tft_executor::window::scale_rect(&window, gold);
    println!("  Gold: ({}, {}) {}x{}", gx, gy, gw, gh);

    // Capture and save debug frame
    match tft_executor::capture::capture_window(&window) {
        Ok(img) => {
            let debug_dir = std::path::Path::new("artifacts/captures");
            std::fs::create_dir_all(debug_dir)?;
            let path = debug_dir.join("calibrate_frame.png");
            img.save(&path)?;
            println!("\nDebug frame saved to: {}", path.display());

            for (i, region) in regions.iter().enumerate() {
                let (x, y, w, h) = tft_executor::window::scale_rect(&window, *region);
                let cropped = tft_executor::capture::crop_region(&img, x, y, w, h);
                let crop_path = debug_dir.join(format!("slot_{}.png", i));
                cropped.save(&crop_path)?;
                println!("  Slot {} crop: {}", i, crop_path.display());
            }

            let (gx, gy, gw, gh) = tft_executor::window::scale_rect(&window, gold);
            let gold_crop = tft_executor::capture::crop_region(&img, gx, gy, gw, gh);
            let gold_path = debug_dir.join("gold_region.png");
            gold_crop.save(&gold_path)?;
            println!("  Gold crop: {}", gold_path.display());
        }
        Err(e) => {
            println!("Capture failed: {}", e);
        }
    }

    Ok(())
}
