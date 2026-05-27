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

## M4 Checklist (future)

- [ ] Phase detection (LCU or OCR) working in loop
- [ ] Full 35-action set with legal_mask per phase
- [ ] Redline triggers correctly on consecutive failures
- [ ] `run-match` completes a full game without panic
- [ ] Curriculum stages 1-3 each run on real client
