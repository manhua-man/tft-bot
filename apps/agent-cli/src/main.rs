use std::io::{self, BufRead, Write};

mod onnx_infer;
mod run_afk;
mod run_match;

use tft_env::sim_env::SimEnv;
use tft_env::{DiscreteAction, EpisodeOutcome, StepResult, TftEnv};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse args manually: expect "sim-env --seed N [--max-rounds M]"
    let mut seed: u64 = 0;
    let mut max_rounds: u8 = 6;
    let mut mode = "";
    let mut model_path = String::new();
    let mut max_steps: usize = 100;
    let mut trajectory_path = String::new();
    let mut report_path = String::new();
    let mut queue_id: u32 = 1090;
    let mut max_games: u32 = 0;
    let mut policy = "onnx";

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "sim-env" => {
                mode = "sim-env";
            }
            "run-bot" => {
                mode = "run-bot";
            }
            "run-match" => {
                mode = "run-match";
            }
            "run-afk" => {
                mode = "run-afk";
            }
            "--seed" => {
                i += 1;
                if i < args.len() {
                    seed = args[i].parse().expect("invalid seed value");
                }
            }
            "--max-rounds" => {
                i += 1;
                if i < args.len() {
                    max_rounds = args[i].parse().expect("invalid max-rounds value");
                }
            }
            "--model" => {
                i += 1;
                if i < args.len() {
                    model_path = args[i].clone();
                }
            }
            "--max-steps" => {
                i += 1;
                if i < args.len() {
                    max_steps = args[i].parse().expect("invalid max-steps value");
                }
            }
            "--trajectory" => {
                i += 1;
                if i < args.len() {
                    trajectory_path = args[i].clone();
                }
            }
            "--report" => {
                i += 1;
                if i < args.len() {
                    report_path = args[i].clone();
                }
            }
            "--queue-id" => {
                i += 1;
                if i < args.len() {
                    queue_id = args[i].parse().expect("invalid queue-id value");
                }
            }
            "--games" => {
                i += 1;
                if i < args.len() {
                    max_games = args[i].parse().expect("invalid games value");
                }
            }
            "--policy" => {
                i += 1;
                if i < args.len() {
                    policy = Box::leak(args[i].clone().into_boxed_str());
                }
            }
            "--help" | "-h" => {
                eprintln!("Usage: agent-cli <mode> [options]");
                eprintln!();
                eprintln!("Modes:");
                eprintln!("  sim-env    Sim environment (JSON Lines protocol)");
                eprintln!("  run-bot    Real machine bot (ONNX inference + executor)");
                eprintln!("  run-match  Full match autopilot (preflight + phase + redline)");
                eprintln!("  run-afk    Full AFK loop (lobby -> game -> lobby, multi-game)");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  --seed N         Random seed (sim-env)");
                eprintln!("  --max-rounds M   Max rounds per episode (sim-env, default: 6)");
                eprintln!("  --model PATH     ONNX model path (run-bot, run-match)");
                eprintln!("  --max-steps N    Max steps (default: 100)");
                eprintln!("  --trajectory P   Trajectory JSONL output path");
                eprintln!("  --report P       Run report JSON output path (run-match, run-afk)");
                eprintln!("  --queue-id N     TFT queue ID (run-afk, default: 1090=normal)");
                eprintln!("  --games N        Max games to play (run-afk, default: unlimited)");
                eprintln!("  --policy P       Policy: onnx (default) or rule (run-afk)");
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    if mode == "sim-env" {
        run_sim_env(seed, max_rounds);
    } else if mode == "run-bot" {
        if model_path.is_empty() {
            eprintln!("Error: --model PATH required for run-bot mode");
            std::process::exit(1);
        }
        if let Err(e) = run_bot(&model_path, max_steps, &trajectory_path) {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    } else if mode == "run-match" {
        if model_path.is_empty() {
            eprintln!("Error: --model PATH required for run-match mode");
            std::process::exit(1);
        }
        if let Err(e) =
            run_match::run_match(&model_path, max_steps, &trajectory_path, &report_path)
        {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    } else if mode == "run-afk" {
        if model_path.is_empty() && policy == "onnx" {
            eprintln!("Error: --model PATH required for onnx policy (use --policy rule for rule-based)");
            std::process::exit(1);
        }
        if let Err(e) = run_afk::run_afk(
            &model_path,
            max_steps,
            &trajectory_path,
            &report_path,
            queue_id,
            max_games,
            policy,
        ) {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    } else {
        std::process::exit(1);
    }
}

fn run_sim_env(seed: u64, max_rounds: u8) {
    let mut env = SimEnv::new(max_rounds);
    let obs = env.reset(seed);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Send initial reset observation
    emit_json(
        &mut out,
        &ResetMsg {
            msg_type: "reset",
            obs: &obs,
        },
    );

    // Read stdin line by line
    let stdin = io::stdin();
    let mut next_seed = seed.wrapping_add(1);
    let mut current_seed = seed;
    let mut episode_reward = 0.0f32;
    let mut episode_steps = 0usize;

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // EOF
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Handle special command: "reset"
        if trimmed == "reset" {
            let obs = env.reset(next_seed);
            current_seed = next_seed;
            next_seed = next_seed.wrapping_add(1);
            episode_reward = 0.0;
            episode_steps = 0;
            emit_json(
                &mut out,
                &ResetMsg {
                    msg_type: "reset",
                    obs: &obs,
                },
            );
            continue;
        }

        // Handle special command: "reset SEED"
        if let Some(rest) = trimmed.strip_prefix("reset ") {
            let new_seed: u64 = match rest.trim().parse() {
                Ok(s) => s,
                Err(_) => {
                    eprintln!("Invalid seed in reset command: {}", rest);
                    continue;
                }
            };
            let obs = env.reset(new_seed);
            current_seed = new_seed;
            next_seed = new_seed.wrapping_add(1);
            episode_reward = 0.0;
            episode_steps = 0;
            emit_json(
                &mut out,
                &ResetMsg {
                    msg_type: "reset",
                    obs: &obs,
                },
            );
            continue;
        }

        // Parse action id
        let action_id: u16 = match trimmed.parse() {
            Ok(id) => id,
            Err(_) => {
                eprintln!(
                    "Invalid input (expected action id 0..{}): {}",
                    DiscreteAction::count() - 1,
                    trimmed
                );
                continue;
            }
        };

        let action = match DiscreteAction::from_u16(action_id) {
            Some(a) => a,
            None => {
                eprintln!(
                    "Action id {} out of range (0..{})",
                    action_id,
                    DiscreteAction::count() - 1
                );
                continue;
            }
        };

        // Execute step
        let result = env.step(action);
        episode_reward += result.reward;
        episode_steps += 1;

        emit_json(
            &mut out,
            &StepMsg {
                msg_type: "step",
                result: &result,
            },
        );

        // If terminated/truncated, send outcome
        if result.terminated || result.truncated {
            let placement = result
                .info
                .get("placement")
                .and_then(|v| v.as_f64())
                .unwrap_or(8.0) as f32;
            let final_hp = result
                .info
                .get("final_hp")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u16;
            let round_survived = result
                .info
                .get("round")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u8;

            let outcome = EpisodeOutcome {
                seed: current_seed,
                total_reward: episode_reward,
                steps: episode_steps,
                placement,
                final_hp,
                round_survived,
            };
            emit_json(
                &mut out,
                &OutcomeMsg {
                    msg_type: "outcome",
                    outcome: &outcome,
                },
            );
        }
    }
}

// -- Helper ------------------------------------------------------------------

fn emit_json<W: Write, T: serde::Serialize>(out: &mut W, value: &T) {
    writeln!(out, "{}", serde_json::to_string(value).unwrap()).unwrap();
    out.flush().unwrap();
}

// -- Message types for JSON serialization ------------------------------------

#[derive(serde::Serialize)]
struct ResetMsg<'a> {
    #[serde(rename = "type")]
    msg_type: &'a str,
    obs: &'a tft_env::Obs,
}

#[derive(serde::Serialize)]
struct StepMsg<'a> {
    #[serde(rename = "type")]
    msg_type: &'a str,
    result: &'a StepResult,
}

#[derive(serde::Serialize)]
struct OutcomeMsg<'a> {
    #[serde(rename = "type")]
    msg_type: &'a str,
    outcome: &'a EpisodeOutcome,
}

// -- run-bot mode ---------------------------------------------------------------

fn run_bot(model_path: &str, max_steps: usize, trajectory_path: &str) -> anyhow::Result<()> {
    use onnx_infer::OnnxPolicy;
    use tft_domain::UserPreset;
    use tft_executor::backend::ExecutorBackend;
    use tft_env::real_env::RealEnv;

    eprintln!("[run-bot] Loading ONNX model from {model_path}...");
    let mut policy = OnnxPolicy::load(model_path)?;
    eprintln!("[run-bot] Model loaded. Starting real-machine loop (max_steps={max_steps}).");

    let traj = if trajectory_path.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(trajectory_path))
    };

    // Build the best available backend (real or stub)
    let corrections = ExecutorBackend::load_corrections();
    let backend = ExecutorBackend::build_with_corrections(corrections)?;
    eprintln!(
        "[run-bot] Backend: {}",
        if backend.is_real { "REAL" } else { "STUB" }
    );

    let mut env = RealEnv::new(
        backend.discovery,
        backend.ocr,
        backend.input,
        backend.corrections,
        UserPreset::default(),
        max_steps,
        traj,
    );

    let obs = env.reset(0);
    eprintln!("[run-bot] Initial observation received. Entering control loop.");

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut total_reward = 0.0f32;
    let mut step = 0usize;

    let mut current_obs = obs;
    loop {
        // Infer action from ONNX model
        let mask = env.legal_mask();
        let action_id = policy.predict_action_masked(&current_obs, &mask)?;
        let action = DiscreteAction::from_u16(action_id).unwrap_or(DiscreteAction::Noop);

        // Execute
        let result = env.step(action);
        total_reward += result.reward;
        step += 1;

        emit_json(
            &mut out,
            &StepMsg {
                msg_type: "step",
                result: &result,
            },
        );

        if result.terminated || result.truncated {
            let outcome = EpisodeOutcome {
                seed: 0,
                total_reward,
                steps: step,
                placement: 0.0, // unknown in real mode
                final_hp: 0,
                round_survived: step as u8,
            };
            emit_json(
                &mut out,
                &OutcomeMsg {
                    msg_type: "outcome",
                    outcome: &outcome,
                },
            );
            break;
        }

        current_obs = result.obs;
    }

    eprintln!("[run-bot] Done. {step} steps, total_reward={total_reward:.2}");
    Ok(())
}
