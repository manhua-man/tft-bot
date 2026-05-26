# Real Machine SOP (M2)

## Prerequisites

- Windows 10/11 with TFT game installed
- Game running at 1024x768 or higher resolution
- Rust toolchain with MSVC (see `scripts/with-msvc.cmd`)
- `tft-executor` and `executor-probe` built

## Build

```bash
# From F:/tft-bot
scripts/with-msvc.cmd cargo build -p tft-executor -p executor-probe
```

## Preflight Check

```bash
executor-probe preflight
```

Expected output:
```
=== Preflight Check ===
Window: <TFT window title> (<width>x<height>)
  Position: (<left>, <top>)
Capture: OK
OCR: OK
Input: OK
```

All checks must pass before proceeding. If any fail:
- Window not found: ensure game is running and window is visible
- Capture fails: check screen permissions, multi-monitor setup
- OCR fails: check OCR engine availability
- Input fails: run as administrator (SendInput requires elevation)

## Shop Read Test

```bash
executor-probe read-shop
```

Expected: JSON array of 5 shop slot readouts with corrected unit names.

## Single Buy Verification

```bash
executor-probe buy --slot 2
```

Expected output:
```
Before: gold=<N>, shop=["<name0>", "<name1>", "<name2>", "<name3>", "<name4>"]
Sent buy command for slot 2
After: gold=<N-price>
Effect verified: true
  gold_changed: true
  slot_changed: true
```

### EffectVerified Criteria

A buy action is considered verified if ANY of:
1. **gold_changed**: gold value decreased (slot cost deducted)
2. **slot_changed**: the shop slot text changed (unit was purchased)

If neither condition is met within 300ms, the action is marked as NOT verified.

## Failure Handling

**FAIL-FAST rules:**
1. If preflight fails → stop, do not attempt actions
2. If buy returns `effect_verified: false` → stop, do not retry
3. If OCR returns empty/garbage for all 5 slots → stop, OCR not working
4. If window disappears mid-action → stop, game may have closed

Do NOT expand action set beyond shop buy until single-buy verification is stable.

## Evidence Collection

All actions are logged with:
- Timestamp
- Before/after state (gold, shop slots, bench count)
- EffectVerified result
- Screenshots (if artifact policy enabled)

Logs go to `F:/tft-bot/data/executor-logs/`.

## Troubleshooting

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| Window not found | Game minimized or title changed | Ensure game window is visible |
| Gold always 0 | OCR region off for current resolution | Check `shop_slot_regions()` coordinates |
| effect_verified always false | Input not reaching game | Run as admin, check window focus |
| OCR returns garbage | Game window partially obscured | Ensure clean game window |
