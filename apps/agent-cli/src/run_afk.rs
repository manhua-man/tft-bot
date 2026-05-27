//! run-afk — Full AFK loop: meta FSM → in-game loop → next game.
//!
//! Uses tft-meta to automate lobby → accept → loading → running → end → lobby.
//! Each game runs the in-game loop from run_match::run_ingame_loop.
//!
//! Supports two policies:
//! - `onnx`: Uses ONNX model for inference (default)
//! - `rule`: Uses rule-based shop policy (cheapest first, no model needed)

use anyhow::Result;
use tft_executor::lcu_gate::MetaMode;
use tft_meta::config::MetaConfig;
use tft_meta::fsm::{GameOutcome, MetaFsm};

use super::run_match;

/// Run the full AFK loop.
pub fn run_afk(
    model_path: &str,
    max_steps: usize,
    trajectory_path: &str,
    report_path: &str,
    queue_id: u32,
    max_games: u32,
    policy: &str,
) -> Result<()> {
    // Resolve meta mode
    let lockfile_path = std::env::var("LCU_LOCKFILE")
        .unwrap_or_else(|_| tft_executor::lcu_gate::DEFAULT_LOCKFILE_PATH.to_string());

    let meta_mode = MetaMode::from_env();
    if meta_mode == MetaMode::Lcu {
        let probe = tft_executor::lcu_gate::probe_lcu(&lockfile_path);
        if !probe.available {
            anyhow::bail!(
                "TFT_META_MODE=lcu but LCU not available (lockfile?). See docs/LCU_CN.md — use manual or fix LCU_LOCKFILE"
            );
        }
    }

    // Validate: onnx policy requires a model path
    if policy == "onnx" && model_path.is_empty() {
        anyhow::bail!("--model PATH required for onnx policy (or use --policy rule)");
    }

    let config = MetaConfig {
        queue_id,
        meta_mode,
        lockfile_path,
        ..Default::default()
    };

    let mut fsm = MetaFsm::new(config);
    fsm.max_games = max_games;

    eprintln!(
        "[run-afk] Starting AFK loop: mode={}, queue_id={}, max_games={}, max_steps={}, policy={}",
        meta_mode, queue_id, max_games, max_steps, policy
    );

    let model = model_path.to_string();
    let traj = trajectory_path.to_string();
    let policy_name = policy.to_string();

    let outcomes = fsm.run(|_lcu_client| {
        eprintln!("[run-afk] Game started, policy={}...", policy_name);

        let result = if policy_name == "rule" {
            run_match::run_ingame_loop_rule(&traj, max_steps)?
        } else {
            run_match::run_ingame_loop(&model, max_steps, &traj)?
        };

        Ok(GameOutcome {
            steps: result.steps,
            total_reward: result.total_reward,
            placement: None,
            redline_triggered: result.redline_reason.is_some(),
            redline_reason: result.redline_reason,
            verified_buys: result.verified_buys,
            failed_buys: result.failed_buys,
            augment_clicks: result.augment_clicks,
            phase_changes: result.phase_changes,
        })
    })?;

    // Write summary report
    let run_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let report = serde_json::json!({
        "run_id": run_id,
        "mode": meta_mode.to_string(),
        "policy": policy,
        "queue_id": queue_id,
        "games_played": outcomes.len(),
        "games": outcomes.iter().enumerate().map(|(i, o)| {
            serde_json::json!({
                "game": i + 1,
                "steps": o.steps,
                "total_reward": o.total_reward,
                "placement": o.placement,
                "redline_triggered": o.redline_triggered,
                "redline_reason": o.redline_reason,
                "verified_buys": o.verified_buys,
                "failed_buys": o.failed_buys,
                "augment_clicks": o.augment_clicks,
                "phase_changes": o.phase_changes.iter().map(|(s, p)| {
                    serde_json::json!({"step": s, "phase": p})
                }).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
        "total_steps": outcomes.iter().map(|o| o.steps).sum::<usize>(),
        "total_reward": outcomes.iter().map(|o| o.total_reward).sum::<f32>(),
        "total_verified_buys": outcomes.iter().map(|o| o.verified_buys).sum::<usize>(),
        "total_failed_buys": outcomes.iter().map(|o| o.failed_buys).sum::<usize>(),
        "total_augment_clicks": outcomes.iter().map(|o| o.augment_clicks).sum::<usize>(),
        "avg_steps_per_game": if outcomes.is_empty() { 0.0 } else { outcomes.iter().map(|o| o.steps).sum::<usize>() as f32 / outcomes.len() as f32 },
        "avg_reward_per_game": if outcomes.is_empty() { 0.0 } else { outcomes.iter().map(|o| o.total_reward).sum::<f32>() / outcomes.len() as f32 },
        "redline_triggered_count": outcomes.iter().filter(|o| o.redline_triggered).count(),
        "timestamp": run_id,
    });

    let report_path = if report_path.is_empty() {
        format!("artifacts/reports/afk-{}.json", run_id)
    } else {
        report_path.to_string()
    };

    if let Some(parent) = std::path::Path::new(&report_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
    eprintln!("[run-afk] Report saved to {}", report_path);

    Ok(())
}
