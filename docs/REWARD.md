# Reward Specification (v1)

## Design Principles

1. **Verifiable**: every reward signal traces to a concrete game state change.
2. **Hack-resistant**: terminal reward dominates; step rewards are shaping only.
3. **Versioned**: this file is the contract; changes require bumping the version header.

## Version History

| Version | Date | Change |
|---------|------|--------|
| v1.0 | 2026-05 | Initial: step shaping + terminal placement |

---

## Step Reward (per action)

Awarded immediately after each `DiscreteAction` is applied.

| Signal | Value | Trigger |
|--------|-------|---------|
| Buy preferred unit | +2.2 | Shop slot matches `user_preset.desired_units` |
| Buy non-preferred unit | +0.6 | Any other valid buy |
| Reroll (no preferred hit in shop) | +0.9 | Shop had no preferred units before reroll |
| Reroll (had preferred hit) | -0.2 | Rerolled away from a preferred shop slot |
| Buy XP / Level up | +1.1 | Standard level-up |
| Level up with 46+ gold banked | +2.0 | Economy-aware level-up |
| Promote bench → board | +2.4 | Unit moved from bench to open board slot |
| Fill board (per unit) | +2.4 | FillBoard action, per unit placed |
| Sell weakest | +0.5 | Bench unit sold for gold |
| Choose preferred augment | +1.8 | Augment matches `user_preset.augment_priority` |
| Choose non-preferred augment | +1.1 | Any augment chosen |
| Noop (no penalty case) | +0.1 | Hold when no obvious action |
| Noop blunder: preferred shop hit ignored | -1.0 | ShopEconomy phase, preferred unit available |
| Noop blunder: bench unit left unplayed | -1.0 | BoardPlacement/Combat, board has gap, bench non-empty |
| Illegal action | -1.0 | Insufficient gold, full bench, wrong phase, etc. |
| End-of-round noise | +0.0..0.35 | Small random bonus per round (matches tft-sim) |
| Board gap penalty | -0.8 per gap | Per empty board slot vs level (end-of-round) |
| Win streak bonus | +0.8 | No board gap at end-of-round |

### Notes

- Step rewards are **shaping signals**. The agent should NOT learn to maximize them at the expense of terminal reward.
- Illegal actions get -1.0 penalty (hard penalty, not just masking). The `legal_mask()` is also available but the penalty ensures the agent learns to avoid illegal moves even without masking.

---

## Terminal Reward (end of episode)

Awarded once when `terminated=true` or `truncated=true`.

### Placement Formula

```
placement = clamp(8.5 - ((strength_score + health / 25.0) / 3.1), 1.0, 8.0)
terminal_reward = (4.5 - placement) * 2.0
```

| Placement | Terminal Reward |
|-----------|----------------|
| 1st | +7.0 |
| 2nd | +5.0 |
| 3rd | +3.0 |
| 4th | +1.0 |
| 5th | -1.0 |
| 6th | -3.0 |
| 7th | -5.0 |
| 8th | -7.0 |

### Strength Score Components

The `strength_score` accumulates throughout the episode:

- Action score deltas (from step rewards, excluding noise)
- Win streak bonus (+0.8 per round with no board gap)
- Board gap penalty (-0.8 per gap per round)
- Random noise per round (+0.0..0.35)

---

## Episode Return

```
episode_return = sum(step_rewards) + terminal_reward
```

Typical range: roughly -20 to +40 over 6 rounds.

---

## M1 Success Criteria

On 32 fixed seeds (42..73), with max_rounds=6:

| Agent | Mean Episode Return | Mean Placement |
|-------|-------------------|----------------|
| Random | baseline | baseline |
| RuleTeacher | > Random + 2.0 | < Random placement |
| PPO (trained) | > RuleTeacher + 1.0 | < RuleTeacher placement |

---

## Future Versions (M3+)

- **Sparse terminal only**: remove step shaping for real-machine env (prevents reward hacking).
- **Shaped terminal**: add board strength heuristics as terminal bonus.
- **Curriculum**: start with step shaping, anneal to sparse over training.
