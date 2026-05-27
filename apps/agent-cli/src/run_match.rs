use anyhow::Result;
use tft_env::{DiscreteAction, TftEnv};

/// Run a single in-game loop (shared by run-match and run-afk).
///
/// Returns: (steps, total_reward, redline_reason, phase_changes)
pub fn run_ingame_loop(
    model_path: &str,
    max_steps: usize,
    trajectory_path: &str,
) -> Result<InGameResult> {
    use super::onnx_infer::OnnxPolicy;
    use tft_domain::UserPreset;
    use tft_env::real_env::RealEnv;
    use tft_env::redline::{RedlineConfig, RedlineMonitor};
    use tft_env::CurriculumPhase;
    use tft_executor::backend::ExecutorBackend;
    use tft_executor::lcu_gate::LcuGate;
    use tft_executor::phase::{AgentPhase, PhaseDetector, PhaseDetectorConfig};

    // 1. Build backend
    let corrections = ExecutorBackend::load_corrections();
    let backend = ExecutorBackend::build_with_corrections(corrections)?;
    if backend.is_real {
        eprintln!("[ingame] Backend: REAL");
    } else {
        eprintln!("[ingame] Backend: STUB");
    }

    // 2. Preflight: LCU probe
    let lockfile_path = std::env::var("LCU_LOCKFILE")
        .unwrap_or_else(|_| tft_executor::lcu_gate::DEFAULT_LOCKFILE_PATH.to_string());
    let lcu_gate = LcuGate::probe(&lockfile_path);
    if lcu_gate.is_available() {
        eprintln!("[ingame] LCU: available");
    } else {
        eprintln!("[ingame] LCU: unavailable");
    }

    // 3. Phase detector
    let phase_config = PhaseDetectorConfig {
        lockfile_path: lockfile_path.clone(),
        ..Default::default()
    };
    let mut phase_detector = PhaseDetector::new(phase_config);
    eprintln!(
        "[ingame] Phase detector: LCU={}, initial={}",
        phase_detector.is_lcu_available(),
        phase_detector.current_phase()
    );

    // 4. Load ONNX model
    eprintln!("[ingame] Loading ONNX model from {model_path}...");
    let mut policy = OnnxPolicy::load(model_path)?;
    eprintln!("[ingame] Model loaded.");

    // 5. Create RealEnv with full curriculum
    let traj = if trajectory_path.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(trajectory_path))
    };

    let mut env = RealEnv::new(
        backend.discovery,
        backend.ocr,
        backend.input,
        backend.corrections,
        UserPreset::default(),
        max_steps,
        traj,
    );
    env.set_curriculum_phase(CurriculumPhase::ShopEconomy);

    // 6. Redline monitor
    let redline_config = RedlineConfig {
        max_consecutive_blunders: 10,
        max_steps_without_progress: 30,
        ..Default::default()
    };
    let mut redline = RedlineMonitor::new(redline_config);

    // 7. Main loop
    let mut obs = env.reset(0);
    let mut step = 0usize;
    let mut total_reward = 0.0f32;
    let mut redline_reason: Option<String> = None;
    let mut phase_changes: Vec<(usize, String)> = Vec::new();

    eprintln!("[ingame] Entering control loop (max_steps={max_steps}).");

    loop {
        // Update phase
        phase_detector.update_lcu();
        let current_phase = phase_detector.current_phase();

        // Log phase changes
        if phase_changes.is_empty()
            || phase_changes.last().map(|(_, p)| p.as_str()) != Some(&current_phase.to_string())
        {
            phase_changes.push((step, current_phase.to_string()));
            eprintln!("[ingame] Step {}: phase={}", step, current_phase);
        }

        // Check if we can act
        if !current_phase.is_in_game() {
            eprintln!(
                "[ingame] Waiting for game phase (current={})...",
                current_phase
            );
            std::thread::sleep(std::time::Duration::from_secs(2));
            phase_detector.update_lcu();
            // Safety: bail if we've waited too long without entering a game
            if step == 0 && phase_detector.current_phase() == AgentPhase::Idle {
                if phase_changes.len() > 1 {
                    let first_ts = phase_changes.first().map(|(s, _)| *s).unwrap_or(0);
                    if step.saturating_sub(first_ts) as u64 > 3000 {
                        eprintln!("[ingame] Timeout waiting for game phase.");
                        break;
                    }
                }
            }
            continue;
        }

        // Safety: independent max-steps guard
        if step >= max_steps {
            break;
        }

        // Infer action
        let mask = env.legal_mask();
        let action_id = policy.predict_action_masked(&obs, &mask)?;
        let action = DiscreteAction::from_u16(action_id).unwrap_or(DiscreteAction::Noop);

        // Execute
        let result = env.step(action);
        total_reward += result.reward;
        step += 1;

        // Check redline (pass u16::MAX for unknown health to avoid false trigger)
        if let Some(reason) = redline.check(u16::MAX, result.reward, total_reward) {
            redline_reason = Some(reason);
            eprintln!(
                "[ingame] REDLINE triggered at step {}: {}",
                step,
                redline_reason.as_ref().unwrap()
            );
            break;
        }

        if result.terminated || result.truncated {
            break;
        }

        obs = result.obs;
    }

    eprintln!(
        "[ingame] Done. {} steps, total_reward={:.2}",
        step, total_reward
    );

    Ok(InGameResult {
        steps: step,
        total_reward,
        redline_reason,
        phase_changes,
        lcu_available: lcu_gate.is_available(),
    })
}

/// Result of a single in-game run.
pub struct InGameResult {
    pub steps: usize,
    pub total_reward: f32,
    pub redline_reason: Option<String>,
    pub phase_changes: Vec<(usize, String)>,
    pub lcu_available: bool,
}

/// Run a single in-game loop using rule-based shop policy (no ONNX model).
///
/// This is the Phase 2 fallback for testing without a trained model.
/// Uses `RuleShopPolicy::Cheapest` to buy the cheapest available slot.
pub fn run_ingame_loop_rule(trajectory_path: &str, max_steps: usize) -> Result<InGameResult> {
    use tft_domain::UserPreset;
    use tft_env::real_env::RealEnv;
    use tft_env::redline::{RedlineConfig, RedlineMonitor};
    use tft_env::CurriculumPhase;
    use tft_executor::backend::ExecutorBackend;
    use tft_executor::lcu_gate::LcuGate;
    use tft_executor::phase::{AgentPhase, PhaseDetector, PhaseDetectorConfig};
    use tft_meta::rule_shop::RuleShopPolicy;

    let corrections = ExecutorBackend::load_corrections();
    let backend = ExecutorBackend::build_with_corrections(corrections)?;
    eprintln!(
        "[ingame-rule] Backend: {}",
        if backend.is_real { "REAL" } else { "STUB" }
    );

    let lockfile_path = std::env::var("LCU_LOCKFILE")
        .unwrap_or_else(|_| tft_executor::lcu_gate::DEFAULT_LOCKFILE_PATH.to_string());
    let lcu_gate = LcuGate::probe(&lockfile_path);

    let phase_config = PhaseDetectorConfig {
        lockfile_path: lockfile_path.clone(),
        ..Default::default()
    };
    let mut phase_detector = PhaseDetector::new(phase_config);

    let _policy = RuleShopPolicy::Cheapest;

    let traj = if trajectory_path.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(trajectory_path))
    };

    let mut env = RealEnv::new(
        backend.discovery,
        backend.ocr,
        backend.input,
        backend.corrections,
        UserPreset::default(),
        max_steps,
        traj,
    );
    env.set_curriculum_phase(CurriculumPhase::ShopEconomy);

    let redline_config = RedlineConfig {
        max_consecutive_blunders: 10,
        max_steps_without_progress: 30,
        ..Default::default()
    };
    let mut redline = RedlineMonitor::new(redline_config);

    let mut obs = env.reset(0);
    let mut step = 0usize;
    let mut total_reward = 0.0f32;
    let mut redline_reason: Option<String> = None;
    let mut phase_changes: Vec<(usize, String)> = Vec::new();
    let mut verified_buys = 0usize;
    let mut failed_buys = 0usize;

    eprintln!("[ingame-rule] Entering control loop (max_steps={max_steps}).");

    loop {
        phase_detector.update_lcu();
        let current_phase = phase_detector.current_phase();

        if phase_changes.is_empty()
            || phase_changes.last().map(|(_, p)| p.as_str()) != Some(&current_phase.to_string())
        {
            phase_changes.push((step, current_phase.to_string()));
            eprintln!("[ingame-rule] Step {}: phase={}", step, current_phase);
        }

        if !current_phase.is_in_game() {
            std::thread::sleep(std::time::Duration::from_secs(2));
            phase_detector.update_lcu();
            continue;
        }

        if step >= max_steps {
            break;
        }

        // Use rule policy to choose action based on shop observation
        let action = if current_phase.can_shop() {
            // Read shop from observation (slot texts are in the obs vector)
            // For rule policy, we use the legal_mask to find available buy actions
            let mask = env.legal_mask();
            // Try cheapest first: BuySlot0-4 are indices 1-5
            let mut chosen = tft_env::DiscreteAction::Noop;
            for slot in 0..5u16 {
                if mask.get(slot as usize + 1).copied().unwrap_or(false) {
                    chosen = tft_env::DiscreteAction::from_u16(slot + 1)
                        .unwrap_or(tft_env::DiscreteAction::Noop);
                    break;
                }
            }
            chosen
        } else if current_phase == AgentPhase::Augment {
            // Default: pick center augment (slot 1)
            tft_env::DiscreteAction::Noop // TODO: click augment slot 1
        } else {
            tft_env::DiscreteAction::Noop
        };

        let result = env.step(action);
        total_reward += result.reward;
        step += 1;

        // Track buy success
        if matches!(
            action,
            tft_env::DiscreteAction::BuySlot0
                | tft_env::DiscreteAction::BuySlot1
                | tft_env::DiscreteAction::BuySlot2
                | tft_env::DiscreteAction::BuySlot3
                | tft_env::DiscreteAction::BuySlot4
        ) {
            if result.reward > -0.5 {
                verified_buys += 1;
            } else {
                failed_buys += 1;
            }
        }

        if let Some(reason) = redline.check(u16::MAX, result.reward, total_reward) {
            redline_reason = Some(reason);
            eprintln!(
                "[ingame-rule] REDLINE at step {}: {}",
                step,
                redline_reason.as_ref().unwrap()
            );
            break;
        }

        if result.terminated || result.truncated {
            break;
        }

        obs = result.obs;
    }

    eprintln!(
        "[ingame-rule] Done. {} steps, reward={:.2}, buys={}/{}",
        step,
        total_reward,
        verified_buys,
        verified_buys + failed_buys
    );

    Ok(InGameResult {
        steps: step,
        total_reward,
        redline_reason,
        phase_changes,
        lcu_available: lcu_gate.is_available(),
    })
}

/// Full run-match command: single game with report.
pub fn run_match(
    model_path: &str,
    max_steps: usize,
    trajectory_path: &str,
    report_path: &str,
) -> Result<()> {
    let run_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    eprintln!("[run-match] Starting run {run_id}");

    let result = run_ingame_loop(model_path, max_steps, trajectory_path)?;

    // Write report
    let report = serde_json::json!({
        "run_id": run_id,
        "steps": result.steps,
        "total_reward": result.total_reward,
        "redline_reason": result.redline_reason,
        "phase_changes": result.phase_changes.iter().map(|(s, p)| {
            serde_json::json!({"step": s, "phase": p})
        }).collect::<Vec<_>>(),
        "lcu_available": result.lcu_available,
        "model": model_path,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    });

    let report_path = if report_path.is_empty() {
        format!("artifacts/reports/run-{}.json", run_id)
    } else {
        report_path.to_string()
    };

    if let Some(parent) = std::path::Path::new(&report_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
    eprintln!("[run-match] Report saved to {}", report_path);

    Ok(())
}
