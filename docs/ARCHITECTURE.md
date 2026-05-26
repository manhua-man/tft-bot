# Architecture

## Layer Diagram

```
┌─────────────────────────────────────────────┐
│              Python (RL layer)              │
│  train_ppo.py ←→ env_client.py ←→ SB3 PPO  │
└──────────────────┬──────────────────────────┘
                   │ JSON Lines (stdin/stdout)
┌──────────────────┴──────────────────────────┐
│              agent-cli (Rust binary)        │
│  arg parse → SimEnv::reset/step → JSON out  │
└──────────────────┬──────────────────────────┘
                   │
┌──────────────────┴──────────────────────────┐
│              tft-env (Rust crate)           │
│  TftEnv trait  │  SimEnv  │  RealEnv (M3+)  │
│  DiscreteAction│  Obs     │  StepResult     │
└────────┬───────┴────┬─────┴─────────────────┘
         │            │
┌────────┴───┐  ┌─────┴──────┐
│ tft-sim    │  │ tft-domain │
│ simulator  │  │ types, MDP │
│ episode    │  │ aliases    │
└────────────┘  └────────────┘
```

## Data Flow

1. Python calls `env.reset(seed)` → spawns `agent-cli sim-env --seed N`
2. agent-cli initializes `SimEnv`, returns JSON obs
3. Python calls `env.step(action_id)` → writes action ID to stdin
4. agent-cli maps `DiscreteAction` → game logic → returns `{obs, reward, terminated, truncated, info}`
5. On termination, agent-cli sends outcome JSON, Python auto-resets

## Observation Vector (34 dims)

| Range | Field | Description |
|-------|-------|-------------|
| 0-7 | scalars | gold, level, xp, health, streak, round, board_count, bench_count |
| 8-12 | shop_costs | cost per shop slot (0 if empty) |
| 13-17 | shop_preferred | 1.0 if slot matches preset desired unit |
| 18-22 | board_cost_dist | count of board units by cost tier 1-5 |
| 23-29 | phase | one-hot: lobby, augment, shop, placement, combat, post_combat, carousel |
| 30-33 | flags | bench_full, can_level, can_reroll, pending_augment |

## Reward Design

- **Step reward**: score_delta from action application (buying preferred unit = +2.2, noop = +0.1, illegal = blunder penalty)
- **Terminal reward**: placement-based (1st = +8, 8th = -8, linear interpolation)
- See `docs/REWARD.md` for full specification (M1)
