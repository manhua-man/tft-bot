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

## Milestones

| Milestone | Goal | Status |
|-----------|------|--------|
| M0 | Scaffold + Sim Gym + PPO smoke | ✓ |
| M1 | PPO beats RuleTeacher on fixed seeds | planned |
| M2 | Real machine observation + JCCT executor | planned |
| M3 | RealEnv shop-only + ONNX deploy | planned |
| M4 | Full autopilot + sparse RL | planned |

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

## Migrated From

[F:/TFT](../TFT) — read-only source. See [docs/MIGRATION_FROM_TFT.md](docs/MIGRATION_FROM_TFT.md).

## Compliance

Internal research only. Not distributed as an external product.
