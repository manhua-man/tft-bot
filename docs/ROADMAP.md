# Product roadmap (大厅挂机 → 局内买棋 → 真机 RL)

## Status

| Phase | 代码 | 实机验收 |
|-------|------|----------|
| 0 | DONE | **PASS** — 日志提取 LCU auth；排队/接受（[LCU_CN.md](LCU_CN.md)） |
| 1 | DONE | **PASS** — `tft-meta` + `run-afk` 进局 |
| 2 | DONE | **PASS** — rule 买棋循环（如 30 步 reward=18） |
| 3 | DONE | 轨迹字段 + `finetune_real.py`；可按需补多样本评估 |
| 4 | DONE | FSM 结束回环 + `--games N`；可按需补 3 局无人值守签字 |

This document is the **execution-order plan** for the full product. It is separate from:

| Doc | Purpose |
|-----|---------|
| [COMPLETION.md](COMPLETION.md) | **M0–M4** implementation vs acceptance evidence (what is done / what proof is missing) |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Layers, data flow, [end-to-end flowchart](ARCHITECTURE.md#end-to-end-product-flow-大厅挂机--局内买棋--真机-rl) |
| [REAL_MACHINE_SOP.md](REAL_MACHINE_SOP.md) | How to run executor probes on a live client |

**Milestones (M0–M4)** describe *code areas*; **phases (0–4)** describe *what to build and prove next* without training RL on bad observations.

---

## Phase 0 — Environment & gates (~1 week, decide early)

| Feature | Description | Done when |
|---------|-------------|-----------|
| LCU | `lcu-probe`, [LCU_CN.md](LCU_CN.md) | 默认 manual；可读 lockfile 时再测 `lcu` |
| Window / DPI / capture | **1024×768** | preflight 通过，截图像素与窗口一致 |
| Meta 模式 | `TFT_META_MODE` | 默认 `manual`；bot 从 2999/窗口接手 |

**Maps to milestones**: M2 acceptance, M2.5 (meta) prerequisite.

---

## Phase 1 — Lobby → in-game (Helper core, Rust FSM)

Implement a finite state machine in `agent-cli` or a new crate `tft-meta`, aligned with `参考/TFT-Hextech-Helper-main/src-backend/states/`.

| State | Capability | Source |
|-------|------------|--------|
| Start | Backup/apply `game.cfg` (optional) | Files or skip |
| Lobby | `createLobby` + `startMatch` + timeout re-queue | LCU REST |
| LobbyWait | `acceptMatch` (WebSocket or poll ready-check) | LCU |
| GameLoading | Poll `127.0.0.1:2999/.../allgamedata` | In-game API |
| GameRunning | Window `init` (Helper `tftOperator`) | `win_window` |

**Done when**: Without human intervention, **3 consecutive games** go from lobby to **shop UI visible** (logs show phase / 2999 OK / window init).

**Maps to milestones**: **M2.5 (meta)** — not started; see [COMPLETION.md](COMPLETION.md).

---

## Phase 2 — Minimal in-game AFK (Helper ops + verification)

| Feature | v1 | v2 |
|---------|----|----|
| Phase | LCU `InProgress` + OCR round/shop bar | Align with Helper: SHOP / AUGMENT / COMBAT |
| Shop | Rule: lineup table or cheap/random buy | ONNX shop-only policy |
| Buy | `buy` + `effect_verified` | One retry, then redline |
| Augment | Fixed SLOT_2 or random among three | `augment_priority` from configs |
| Combat | Noop or refresh only | Later |

**Done when**: One match with **≥10 auto-buys**, redline not falsely tripped; **≥2/3** augment rounds clicked.

**Maps to milestones**: M2 (executor), M4 (phase/redline/run-match).

---

## Phase 3 — Real-machine RL (after phase 2)

| Feature | Notes |
|---------|--------|
| RealEnv | obs = shop OCR + gold/level (best effort) + phase one-hot |
| Actions | Shop-only first (5 slots + noop), same as M1 Sim |
| Reward | [REWARD.md](REWARD.md) + real `verified_buy` shaping |
| Data | `run-match` → JSONL; offline or small on-policy updates |
| Safety | redline: invalid clicks, gold unchanged, placeholder noise, repeated coords |

**Done when**: One full match → trajectory; offline train → **buy success rate > rule baseline** on same setup (5–10 games sample OK).

**Maps to milestones**: M3, M1 model as init.

---

## Phase 4 — Match end → loop (Helper EndState)

| Feature | Notes |
|---------|--------|
| End detect | LCU `TFT_BATTLE_PASS` / `WaitingForStats` or OCR placement |
| Exit / next | `early-exit` or return lobby → Lobby |
| Long AFK | Queue timeout, dodge re-queue, abort (see Helper `LobbyState`) |

**Done when**: **N=3** unattended games; each has report: time-to-shop, buy count, verify failures, redline stopped (Y/N).

**Maps to milestones**: M4 + M2.5 loop.

---

## Product backlog

### Must have (defines the product)

1. LCU connector + CN lockfile paths  
2. Meta FSM: Lobby → Accept → Loading (2999) → Running  
3. Window discovery + fixed resolution + capture validation  
4. Phase: at least Shop vs non-Shop  
5. `read-shop` / `buy` + `effect_verified` + redline  
6. `run-match` / `run-bot` wiring meta + in-game  
7. RealEnv trajectory + shop-only RL (Sim weights init)  
8. Augment auto-pick (v1 random OK)  
9. End detect + lobby loop  

### Should have (Helper parity)

10. Queue mode (normal/ranked) + timeout re-queue  
11. Auto `game.cfg` resolution (Helper-style)  
12. Augment by lineup priority  

### Later

13. Item combine, positioning, carousel  
14. Electron UI (CLI + logs enough)  
15. Extra modes (Clockwork, etc.)  

---

## Milestone crosswalk (M vs phases)

| Milestone | Phase focus |
|-----------|-------------|
| M0, M1 | Done (Sim RL) — not repeated in phases |
| M2 | Phase 0–2 executor acceptance |
| M2.5 | Phase 1 meta FSM |
| M3 | Phase 3 RealEnv / ONNX on machine |
| M4 | Phase 2–4 autopilot, redline, run-match, end loop |

Update [COMPLETION.md](COMPLETION.md) **Status** when phase **done when** is met, not only when code lands.
