# Migration from F:/TFT

## Path Mapping

| Old Path (F:/TFT) | New Path (F:/tft-bot) | Notes |
|--------------------|----------------------|-------|
| `crates/tft-domain/` | `crates/tft-domain/` | include paths updated |
| `crates/tft-sim/` | `crates/tft-sim/` | unchanged |
| `crates/tft-strategy/` | `crates/tft-strategy/` | unchanged |
| `configs/s16-patch-pack.json` | `configs/s16-patch-pack.json` | copied |
| `configs/ocr-corrections.json` | `configs/ocr-corrections.json` | copied |
| `configs/strategy-templates/` | `configs/strategy-templates/` | copied |
| `参考/.../hex.ts` | `configs/augment-reference-s16.ts` | renamed, no 参考/ dir |
| `scripts/with-msvc.cmd` | `scripts/with-msvc.cmd` | copied |
| `crates/tft-domain/src/lib.rs` line 38 | same | `include_str!("../../../configs/augment-reference-s16.ts")` |

## Not Migrated

- `products/tft-assistant/` — UI product, out of scope
- `products/tft-automation-lab/` — automation lab, out of scope
- `参考/` — third-party reference code, not needed in bot repo
- `.cursor/`, `.kiro/`, `.harness/` — tooling config
- `crates/tft-runtime-win/` — deferred to M2 (subset only)
- `ml/` — old training scripts, replaced by `python/tft_bot_rl/`

## New Crates

| Crate | Purpose |
|-------|---------|
| `tft-env` | TftEnv trait + SimEnv + DiscreteAction + Obs |
| `agent-cli` | JSON Lines binary for Python ↔ Rust communication |
| `tft-eval` | Benchmark framework (M1+, currently stub) |
