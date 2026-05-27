# Real Machine SOP (Phase 0 → Phase 4)

Full product loop (lobby LCU → in-game vision → JSONL → RL): see [ARCHITECTURE.md — End-to-end product flow](ARCHITECTURE.md#end-to-end-product-flow-大厅挂机--局内买棋--真机-rl). This SOP focuses on **in-game** executor acceptance once you are in a match shop phase.

## Prerequisites

- Windows 10/11 with TFT game installed
- Game running at 1024x768 or higher resolution
- Rust toolchain with MSVC (see `scripts/with-msvc.cmd`)
- `tft-executor` and `executor-probe` built with real features

## Build

```bash
# From F:/tft-bot
cargo build -p executor-probe --features win_window,input_sim
cargo build -p lcu-probe
cargo build -p tft-meta
```

## Phase 0: Environment & Gates

### Step 0: LCU Probe

```bash
lcu-probe
```

Expected output:
```json
{
  "lockfile_found": true,
  "lockfile_path_tried": ["G:\\wegameapps\\英雄联盟\\LeagueClient\\lockfile"],
  "lockfile": { "name": "LeagueClient", "pid": 12345, "port": 54321 },
  "endpoints": {
    "gameflow_phase": { "status": 200, "body": "InProgress" },
    "ready_check": { "status": 200, "body": {"state": "InProgress"} }
  },
  "league_process_paths": ["G:\\wegameapps\\...\\LeagueClientUx.exe"],
  "verdict": "LCU_OK (phase=InProgress)"
}
```

默认 `TFT_META_MODE=manual`（见 [LCU_CN.md](LCU_CN.md)）。你排队进局后跑 `run-afk`。

```powershell
$env:TFT_META_MODE = "manual"
```

### Step 0b: In-Game API（进局加载）

```bash
curl -k https://127.0.0.1:2999/liveclientdata/allgamedata
```

### Step 0c: LCU（仅 lockfile 可读时）

```bash
lcu-probe
# 需要自动接受对局且已确认 lockfile 有效：
# lcu-probe --accept --i-know
```

### Phase 0 Acceptance

```bash
# 1. Preflight passes
executor-probe preflight

# 2. LCU probe returns verdict starting with "LCU_OK"
lcu-probe

# 3. read-shop works during shop phase
executor-probe read-shop
```

## Step 0: LCU Probe

```bash
lcu-probe
```

Expected output:
```json
{
  "lockfile_found": true,
  "lockfile": { "name": "LeagueClient", "pid": 12345, "port": 54321 },
  "endpoints": { "gameflow_phase": { "status": 200, "body": "InProgress" } },
  "verdict": "LCU_OK (phase=InProgress)"
}
```

If LCU is unavailable (lockfile empty/missing), the system falls back to visual-only mode.

## Step 1: Preflight Check

```bash
executor-probe preflight
```

Expected output:
```
=== Preflight Check ===
Backend: real

--- LCU ---
Lockfile: OK
  port: 54321, pid: 12345
Phase: InProgress
  can_act: true
Verdict: LCU_OK (phase=InProgress)

--- Window ---
Window: 英雄联盟 (1024x768) at (0,0)
  DPI: 96
Validation: OK
Capture: OK (1024x768)
OCR: 5 valid slots (of 5), all_noise=false
Input: available
```

**FAIL-FAST rules:**
- Window not found → stop, ensure game is running and visible
- Validation errors → stop, check window title/resolution
- Capture fails → check screen permissions, multi-monitor setup
- All slots noise → OCR not working or not in shop phase

## Step 2: Shop Read Test

```bash
executor-probe read-shop
```

Expected: JSON array of 5 shop slot readouts + noise-filtered summary.

## Step 3: Single Buy Verification

```bash
executor-probe buy --slot 2
```

Expected output:
```
[lcu] LCU phase: InProgress (can act)
Before: gold=Some(10), shop=["亚索", "阿卡丽", "永恩", "劫", "卡特"]
Sent buy command for slot 2
After: gold=Some(7)
Effect verified: true
  gold_changed: true
  slot_changed: true
```

### EffectVerified Criteria

A buy action is considered verified if ANY of:
1. **gold_changed**: gold value decreased (slot cost deducted)
2. **slot_changed**: the shop slot text changed (unit was purchased)

If neither condition is met within 300ms, the action is marked as NOT verified.

## Step 4: Calibration (optional)

```bash
executor-probe calibrate
```

Outputs:
- Scaled coordinates for all shop slots + gold region
- Saves debug frames to `artifacts/captures/`
- Saves cropped slot images for visual inspection

## Failure Handling

**FAIL-FAST rules:**
1. If preflight fails → stop, do not attempt actions
2. If LCU phase gate blocks (not InGame) → stop, wait for game to start
3. If buy returns `effect_verified: false` → stop, do not retry
4. If OCR returns empty/garbage for all 5 slots → stop, OCR not working
5. If window disappears mid-action → stop, game may have closed
6. If noise filter rejects target slot → warn but proceed

Do NOT expand action set beyond shop buy until single-buy verification is stable.

## Evidence Collection

All actions are logged with:
- Timestamp
- Before/after state (gold, shop slots, bench count)
- EffectVerified result
- LCU phase (if available)
- Screenshots (if artifact policy enabled)

Logs go to `F:/tft-bot/data/executor-logs/`.

## Troubleshooting

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| Window not found | Game minimized or title changed | Ensure game window is visible |
| Gold always 0 | OCR region off for current resolution | Run `calibrate` to check coordinates |
| effect_verified always false | Input not reaching game | Run as admin, check window focus |
| OCR returns garbage | Game window partially obscured | Ensure clean game window |
| LCU not available | lockfile 空或无效 | 使用 manual + 视觉（见 LCU_CN.md） |
| Phase gate blocks actions | Not in InGame phase | Wait for game to start |
| All slots noise | Not in shop phase or OCR broken | Check if shop UI is visible |

## Sprint 1-4 Verification

### Sprint 1: 买棋可信

```powershell
# Build
cargo build -p agent-cli --release --features win_window,input_sim

# Single game with rule policy
agent-cli run-afk --policy rule --games 1 --max-steps 80 --model dummy --trajectory artifacts/trajectories/s1.jsonl --report artifacts/reports/s1.json
```

**Pass criteria:**
- `verified_buys` >= 10 in report
- Trajectory JSONL has `verified: true` entries matching report count
- No panics or unhandled errors

### Sprint 2: 阶段与海克斯

```powershell
agent-cli run-afk --policy rule --games 1 --max-steps 120 --model dummy --trajectory artifacts/trajectories/s2.jsonl --report artifacts/reports/s2.json
```

**Pass criteria:**
- `phase_changes` in report contains `ShopPhase`, `Combat`, `Augment`
- `augment_clicks` >= 2 (of 3 possible augment rounds)
- No stall redline during Combat phases

### Sprint 3: 多局稳跑

```powershell
agent-cli run-afk --policy rule --games 3 --max-steps 120 --model dummy --trajectory artifacts/trajectories/g3.jsonl --report artifacts/reports/g3.json
```

**Pass criteria:**
- All 3 games complete ingame loop
- `redline_triggered_count` <= 1
- Each game has `verified_buys` in report

### Sprint 4: 真机 RL

```powershell
# 1. Collect rule baseline (5 games)
npm run s4:collect

# 2. Analyze
npm run s4:analyze

# 3. Train ONNX model (requires Python + SB3)
cd python && python -m tft_bot_rl.finetune_real --trajectory ../artifacts/trajectories/batch-rule.jsonl --bc-warmup --epochs 5

# 4. Run ONNX policy
npm run s4:onnx

# 5. Compare
npm run s4:compare
```

**Pass criteria:**
- `onnx buy_success_rate` >= `rule buy_success_rate` + 5% absolute
- OR `onnx verified_buys` mean > `rule verified_buys` mean (sample >= 5 games)

## M4 Checklist

- [x] Phase detection (LCU or OCR) working in loop
- [x] Augment detection via round text OCR (2-1/3-2/4-2)
- [x] Augment click execution (center slot default)
- [x] Phase-aware redline (Combat noops don't stall)
- [x] Redline triggers correctly on consecutive failures
- [x] `run-match` / `run-afk` completes without panic
- [x] Config de-hardcoded (TFT_REPO_ROOT, LCU_LOG_DIR)
- [x] Report includes verified_buys, failed_buys, augment_clicks, phase_changes
- [ ] Real machine: 3 games completed with rule policy
- [ ] Real machine: ONNX policy outperforms rule baseline
