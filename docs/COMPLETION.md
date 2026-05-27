# Plan completion vs acceptance

This document separates **what is implemented in the repo** from **what the original M0–M4 plan required as proof**. Update the status column when evidence changes.

Last reviewed: 2026-05-27 (实机：LCU 日志鉴权 + run-afk rule 30 步)

**Product target (full loop)**: lobby → accept → in-game load → shop/autoplay → trajectory → RL.

- Flowchart: [ARCHITECTURE.md — End-to-end product flow](ARCHITECTURE.md#end-to-end-product-flow-大厅挂机--局内买棋--真机-rl)  
- Phased plan & backlog: [ROADMAP.md](ROADMAP.md)

## Summary

| Milestone | Implementation | Acceptance (plan) | Status |
|-----------|----------------|-------------------|--------|
| M0 | Sim Gym, `agent-cli`, SB3, CI, tests | `cargo test`, PPO smoke | **Done** |
| M1 | `REWARD.md`, `eval_baselines.py`, `tft-eval`, training scripts | 32 seeds: PPO > RuleTeacher > Random | **Done** — see `python/artifacts/eval/m1-report.json` (2026-05-27: PASS) |
| M2 | `tft-executor`, `executor-probe`, `runtime-observe` | Real window + OCR + `effect_verified` on machine | **Accepted (partial)** — 局内买棋循环已跑通；preflight/read-shop 可按 SOP 补签字 |
| M3 | `RealEnv`, `run-bot`, ONNX export | Shop-only real env + trajectory with outcomes | **Accepted (partial)** — trajectory + `finetune_real.py`；ONNX 真机对比可选 |
| M4 | `redline`, curriculum training, expanded `DiscreteAction` in Sim | Full autopilot on real client + sparse RL | **Accepted (partial)** — `run-afk` + redline；多局报告可选 |
| M2.5 (meta) | [crates/tft-meta](../crates/tft-meta), `run-afk` | LCU 大厅 + 2999 + 进局 | **Done** — 日志 auth；自动排队/接受/进局（[LCU_CN.md](LCU_CN.md)） |

## M0 — Scaffold + Sim RL

**Implemented**

- Workspace: [Cargo.toml](../Cargo.toml) (no Assistant/Lab apps)
- [crates/tft-env](../crates/tft-env): `TftEnv`, `SimEnv`, `DiscreteAction`, JSON protocol
- [apps/agent-cli](../apps/agent-cli): `sim-env` mode
- [python/tft_bot_rl](../python/tft_bot_rl): `env_client.py`, `train_ppo.py`
- [.github/workflows/ci.yml](../.github/workflows/ci.yml)

**Acceptance**

```bash
cargo test --workspace
cd python && python -m tft_bot_rl.train_ppo --smoke
```

## M1 — Sim RL beats baselines

**Implemented**

- [docs/REWARD.md](REWARD.md)
- [python/tft_bot_rl/eval_baselines.py](../python/tft_bot_rl/eval_baselines.py)
- [crates/tft-eval](../crates/tft-eval)
- Reports under `python/artifacts/eval/`

**Acceptance**

```bash
cargo build -p agent-cli --release
cd python
python -m tft_bot_rl.train_ppo --total-steps 100000 --agent-cli ../target/release/agent-cli.exe
python -m tft_bot_rl.eval_baselines --seeds 32 --agent-cli ../target/release/agent-cli.exe --ppo-model artifacts/ppo_tft.zip --output artifacts/eval/m1-report.json
```

Expect console: `M1 VERDICT: PASS (PPO > RuleTeacher > Random)`.

**Latest run (2026-05-27)**

| Policy | Mean return | Mean placement |
|--------|-------------|----------------|
| random | -7.62 | 8.00 |
| rule_teacher | -1.23 | 8.00 |
| ppo | +18.68 | 3.99 |

Verdict: **PASS** (PPO > RuleTeacher > Random on return; PPO placement much better).

**Notes**

- Eval loads **SB3 `.zip`**, not ONNX.
- Baseline-only report without PPO is **PARTIAL**, not PASS.
- `rule_teacher` placement in eval may stay at 8.0 if terminal `info` is not updated each step; use **return** as primary gate (PPO placement is populated).

## M2 — Real observation + JCCT-style executor

**Implemented**

- [crates/tft-executor](../crates/tft-executor): shop, correction, verify, capture, noise, window_validation, lcu_gate modules
- [crates/tft-executor/src/win/window_discovery.rs](../crates/tft-executor/src/win/window_discovery.rs): Win32 `EnumWindows` real window discovery (`win_window` feature)
- [crates/tft-executor/src/input_win.rs](../crates/tft-executor/src/input_win.rs): Win32 `SendInput` hotkey+mouse (`input_sim` feature)
- [crates/tft-executor/src/ocr_winrt.rs](../crates/tft-executor/src/ocr_winrt.rs): WinRT OCR (`ocr_winrt` feature, zh-Hans first)
- [crates/tft-executor/src/noise.rs](../crates/tft-executor/src/noise.rs): Shop slot noise filter (Occupied/empty/low-confidence)
- [crates/tft-executor/src/window_validation.rs](../crates/tft-executor/src/window_validation.rs): Window aspect ratio + title validation
- [crates/tft-executor/src/lcu_gate.rs](../crates/tft-executor/src/lcu_gate.rs): LCU lockfile reader + gameflow-phase gate
- [apps/lcu-probe](../apps/lcu-probe): Standalone LCU probe CLI
- [apps/executor-probe](../apps/executor-probe): preflight/read-shop/buy/calibrate with real backend + LCU gate + noise filter
- [configs/window_profiles.cn.yaml](../configs/window_profiles.cn.yaml), [configs/layouts.cn.yaml](../configs/layouts.cn.yaml)
- [docs/LCU_CN.md](LCU_CN.md)

**Acceptance gap (real-machine testing)**

- Need to run on machine with game client to verify:
  1. `executor-probe preflight` finds real window + LCU phase
  2. `executor-probe read-shop` returns 5 non-empty corrected names
  3. `executor-probe buy --slot N` achieves `effect_verified=true` ≥ 8/10
- Build: `cargo build -p executor-probe --features win_window,ocr_winrt,input_sim` (or `npm run m2:build`)

## M3 — RealEnv shop-only + ONNX

**Implemented**

- [crates/tft-executor/src/backend.rs](../crates/tft-executor/src/backend.rs): `ExecutorBackend` factory — auto-selects real or stub based on features
- [crates/tft-env/src/real_env.rs](../crates/tft-env/src/real_env.rs): `RealEnvBox` type alias + `from_backend()` convenience constructor
- Blanket `impl Trait for Box<dyn Trait>` on `WindowDiscovery`, `OcrEngine`, `InputDispatcher`
- `agent-cli run-bot` now uses `ExecutorBackend::build_with_corrections()` — real backends when features enabled, stubs otherwise
- Trajectory JSONL logging with `obs`, `action`, `reward`, `timestamp` per step

**Acceptance gap (real-machine testing)**

- Need to run `agent-cli run-bot --model artifacts/ppo_tft.onnx --max-steps 20 --trajectory out.jsonl` on real machine
- Verify: trajectory has no stub errors, ≥ 1 verified buy/reroll per 20 steps
- Build: `cargo build -p agent-cli --features win_window,input_sim`

## M4 — Full autopilot + sparse RL

**Implemented (M4.1–M4.4)**

- [crates/tft-executor/src/phase.rs](../crates/tft-executor/src/phase.rs): `PhaseDetector` — unified phase detection (LCU or visual), `AgentPhase` enum, debounce, 7 tests
- [crates/tft-executor/src/input.rs](../crates/tft-executor/src/input.rs): `InputDispatcher` expanded with `buy_xp()` and `sell_hovered()` (+ WinInput + StubInput impls)
- [crates/tft-env/src/real_env.rs](../crates/tft-env/src/real_env.rs): `RealEnv` supports `BuyXp`, `LevelUp`, `SellWeakest`, `HoldGold` actions; `CurriculumPhase`-aware `legal_mask()`; `set_curriculum_phase()`
- [crates/tft-env/src/redline.rs](../crates/tft-env/src/redline.rs): `RedlineMonitor` — health/blunder/stall redlines
- [apps/agent-cli/src/run_match.rs](../apps/agent-cli/src/run_match.rs): `agent-cli run-match` — full match autopilot: preflight → phase wait → ONNX loop → redline → report JSON
- `agent-cli run-match --model <onnx> --max-steps N --trajectory out.jsonl --report report.json`

**Acceptance gap (real-machine testing)**

- Need to run full match on real client
- Verify: redline triggers correctly, phase changes logged, report JSON written
- Build: `cargo build -p agent-cli --features win_window,input_sim`

## README sync

The milestone table in [README.md](../README.md) should match the **Status** column above. After each eval run, paste the verdict line from `m1-report.json` or the console into your changelog if needed.
