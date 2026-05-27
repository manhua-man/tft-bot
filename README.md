# tft-bot

Internal research: fully automated TFT bot trained with reinforcement learning.

## Architecture

```
Rust core                    Python RL
─────────                    ─────────
tft-domain  (types, MDP)     train_ppo.py (SB3 PPO)
tft-sim     (simulation)     env_client.py (Gym wrapper)
tft-env     (SimEnv/RealEnv) ─── JSON Lines ───
tft-strategy (rule teacher)  agent-cli (Rust binary)
tft-eval    (benchmarks)
```

**Training loop**: Python Gym env ↔ `agent-cli sim-env` (JSON Lines stdin/stdout) ↔ Rust `tft-env::SimEnv` ↔ `tft-sim` ↔ `tft-domain`.

## Quick Start

```bash
# Build Rust
cargo build --workspace

# Smoke test RL (short run)
cd python && python -m tft_bot_rl.train_ppo --smoke

# Full training
cd python && python -m tft_bot_rl.train_ppo --total-steps 100000

# Run Rust tests
cargo test --workspace
```

## Milestones & roadmap

- **M0–M4** (what is built / what proof is missing): [docs/COMPLETION.md](docs/COMPLETION.md)  
- **Phases 0–4** (recommended build order, lobby → RL): [docs/ROADMAP.md](docs/ROADMAP.md)

| Milestone | Goal | Status |
|-----------|------|--------|
| M0 | Scaffold + Sim Gym + PPO smoke | Done |
| M1 | PPO beats RuleTeacher on 32 fixed seeds | Done (`npm run rl:m1` → `python/artifacts/eval/m1-report.json`) |
| M2 | Real window + OCR + JCCT executor | Implemented; **run real-machine SOP** on CN client |
| M3 | RealEnv shop-only + ONNX `run-bot` | Wired; needs M2 shop read/buy verified on machine |
| M4 | Full autopilot + sparse RL on client | Sim/redline/curriculum only until M2–M3 |

## Docs

- [ROADMAP.md](docs/ROADMAP.md) — phased product plan (meta → in-game → RL → loop)  
- [COMPLETION.md](docs/COMPLETION.md) — milestone acceptance status  
- [STUBS_AND_M2_M4.md](docs/STUBS_AND_M2_M4.md) — what “skeleton + Stub” means (plain language)  
- [REFERENCES.md](docs/REFERENCES.md) — OCR/script bots, RL sims, tools (no dependency on F:/TFT)

## Project Structure

```
F:/tft-bot/
  Cargo.toml              # workspace
  package.json            # npm scripts
  crates/
    tft-domain/           # game types, snapshot, actions, aliases
    tft-sim/              # seeded simulator, episode runner
    tft-strategy/         # rule kernel, learned kernel, phase router
    tft-env/              # TftEnv trait, SimEnv, DiscreteAction, Obs
    tft-eval/             # benchmark (M1+)
    tft-runtime-win/      # M2+: capture, OCR, trajectory
    tft-executor/         # M2+: JCCT execution
  apps/
    agent-cli/            # JSON Lines protocol binary
  python/
    tft_bot_rl/
      env_client.py       # Gymnasium wrapper
      train_ppo.py        # SB3 PPO training
    tests/
    requirements.txt
  configs/
    s16-patch-pack.json   # unit/trait/item data
    s17-patch-pack.json
    ocr-corrections.json
    augment-reference-s16.ts
    strategy-templates/
  docs/
    ARCHITECTURE.md
    REWARD.md
    MIGRATION_FROM_TFT.md
```

## Discrete Action Space (M0)

| ID | Action |
|----|--------|
| 0 | Noop |
| 1-5 | Buy shop slot 0-4 |
| 6 | Reroll |
| 7 | Buy XP |
| 8 | Sell weakest bench unit |
| 9 | Promote best bench to board |
| 10 | Fill board from bench |
| 11-13 | Choose augment 0-2 |
| 14 | Level up |

## History

Code was initially forked from a dual-product TFT monorepo; that tree is **removed**. See [docs/MIGRATION_FROM_TFT.md](docs/MIGRATION_FROM_TFT.md) for a historical path map only.

## Compliance

Internal research only. Not distributed as an external product.
