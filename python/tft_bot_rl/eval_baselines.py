#!/usr/bin/env python3
"""
M1 Eval Baselines: runs Random, RuleTeacher, and (optionally) PPO policies
over 32 fixed seeds on the TFT SimEnv and produces a comparison report.

Usage:
  python eval_baselines.py \
      --seeds 32 \
      --max-rounds 6 \
      --agent-cli path/to/agent-cli \
      --ppo-model path/to/model.zip \
      --output artifacts/eval/m1-report.json
"""

import argparse
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import numpy as np

# ---------------------------------------------------------------------------
# Env wrapper – talks to agent-cli via the JSON /pipe protocol
# ---------------------------------------------------------------------------

class SimEnv:
    """Gym-like wrapper around agent-cli sim-env JSON Lines protocol.

    Spawns `agent-cli sim-env --seed N --max-rounds M` per episode.
    Reads JSON Lines from stdout, writes action IDs to stdin.
    """

    def __init__(self, agent_cli: str, max_rounds: int = 6):
        self.agent_cli = agent_cli
        self.max_rounds = max_rounds
        self._proc = None
        self._obs = None
        self._info = {}
        self._done = False

    def _stop(self):
        if self._proc is not None:
            try:
                self._proc.stdin.close()
                self._proc.terminate()
                self._proc.wait(timeout=5)
            except Exception:
                try:
                    self._proc.kill()
                    self._proc.wait()
                except Exception:
                    pass
            self._proc = None

    def _read_line(self) -> dict:
        line = self._proc.stdout.readline().strip()
        if not line:
            raise RuntimeError("agent-cli produced no output")
        return json.loads(line)

    def _parse_obs(self, obs_dict) -> np.ndarray:
        return np.array(
            obs_dict["scalars"] + obs_dict["shop_costs"]
            + obs_dict["shop_preferred"] + obs_dict["board_cost_dist"]
            + obs_dict["phase"] + obs_dict["flags"],
            dtype=np.float32,
        )

    def reset(self, seed: int):
        self._stop()
        self._proc = subprocess.Popen(
            [self.agent_cli, "sim-env", "--seed", str(seed),
             "--max-rounds", str(self.max_rounds)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1,
        )
        msg = self._read_line()
        self._obs = self._parse_obs(msg["obs"])
        self._info = {}
        self._done = False
        return self._obs, self._info

    def step(self, action: int):
        self._proc.stdin.write(f"{action}\n")
        self._proc.stdin.flush()
        msg = self._read_line()
        result = msg["result"]
        obs = self._parse_obs(result["obs"])
        reward = float(result["reward"])
        terminated = bool(result["terminated"])
        truncated = bool(result["truncated"])
        info = result.get("info", {})
        done = terminated or truncated
        if terminated:
            try:
                outcome_line = self._proc.stdout.readline().strip()
                if outcome_line:
                    outcome = json.loads(outcome_line)
                    info.update(outcome.get("outcome", {}))
            except Exception:
                pass
        self._obs = obs
        self._info = info
        self._done = done
        return obs, reward, done, info

    def close(self):
        self._stop()


# ---------------------------------------------------------------------------
# Policies
# ---------------------------------------------------------------------------

def random_policy(_obs, _info):
    """Pure random: choose uniformly from all 35 actions."""
    return np.random.randint(0, 35)


def rule_teacher_action(obs, info):
    """Simplified RuleTeacher (expanded 35-action space).

    Action layout (35 actions):
      0: Noop
      1-5: BuySlot0..4
      6: Reroll
      7: BuyXp
      8: LevelUp
      9: PromoteBestBench
      10: PromoteBenchSlot0..4 (10-14)
      11: ChooseAugment0
      12: ChooseAugment1
      13: ChooseAugment2
      14: SellSlot0..4 (14-18)
      19-28: MoveBoard positions
      29-33: Item-related actions
      34: Reserved / future
    """
    gold = obs[0]
    level = obs[1]
    shop_preferred = obs[13:18]
    phase = obs[23:30]
    # Augment
    if phase[1] == 1.0:  # augment
        return 11  # ChooseAugment0 (unchanged)
    # Shop / economy phase
    if phase[2] == 1.0:  # shop economy
        # Buy preferred slot if affordable
        for i in range(5):
            if shop_preferred[i] == 1.0:
                return 1 + i  # BuySlot0..4
        # Level up if can afford (LevelUp is now action 8)
        if gold >= 4 and level < 10:
            return 8  # LevelUp
        # Reroll if can afford
        if gold >= 2:
            return 6  # Reroll (unchanged)
    # Board placement phase
    if phase[3] == 1.0:  # placement
        return 9  # PromoteBestBench (unchanged)
    return 0  # Noop


# ---------------------------------------------------------------------------
# Episode runner
# ---------------------------------------------------------------------------

def run_episode(env, policy_fn, seed):
    """Run one episode. Returns (total_reward, placement, steps)."""
    obs, info = env.reset(seed)
    total_reward = 0.0
    steps = 0
    placement = info.get("placement", 8.0)

    while True:
        action = policy_fn(obs, info)
        obs, reward, done, info = env.step(action)
        total_reward += reward
        steps += 1
        placement = info.get("placement", placement)
        if done:
            break

    return total_reward, placement, steps


def evaluate_policy(env, policy_fn, name, seeds):
    """Evaluate a policy over multiple seeds. Returns list of dicts."""
    results = []
    for seed in seeds:
        ret, place, steps = run_episode(env, policy_fn, seed)
        results.append({"seed": int(seed), "return": round(ret, 4),
                        "placement": round(place, 4), "steps": int(steps)})
    return results


# ---------------------------------------------------------------------------
# PPO wrapper
# ---------------------------------------------------------------------------

def make_ppo_policy(model_path):
    """Return a policy function backed by a trained PPO model."""
    try:
        from stable_baselines3 import PPO
        model = PPO.load(model_path)
    except Exception as e:
        print(f"[WARN] Could not load PPO model from {model_path}: {e}")
        return None

    def ppo_policy(obs, _info):
        action, _ = model.predict(obs, deterministic=True)
        return int(action)

    return ppo_policy


# ---------------------------------------------------------------------------
# Stats helpers
# ---------------------------------------------------------------------------

def stats(per_seed, key):
    vals = [r[key] for r in per_seed]
    arr = np.array(vals, dtype=np.float64)
    return {
        f"mean_{key}": round(float(arr.mean()), 4),
        f"std_{key}": round(float(arr.std()), 4),
        f"min_{key}": round(float(arr.min()), 4),
        f"max_{key}": round(float(arr.max()), 4),
    }


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="M1 Eval Baselines")
    parser.add_argument("--seeds", type=int, default=32,
                        help="Number of evaluation seeds (default 32, seeds 42..73)")
    parser.add_argument("--max-rounds", type=int, default=6)
    parser.add_argument("--agent-cli", type=str, required=True,
                        help="Path to agent-cli binary")
    parser.add_argument("--ppo-model", type=str, default=None,
                        help="Path to trained PPO model zip (optional)")
    parser.add_argument("--output", type=str, default="artifacts/eval/m1-report.json")
    args = parser.parse_args()

    seeds = list(range(42, 42 + args.seeds))
    env = SimEnv(args.agent_cli, max_rounds=args.max_rounds)

    report_policies = {}

    # ---- Random ----
    print(f"[eval] Running Random over {len(seeds)} seeds ...")
    rand_results = evaluate_policy(env, random_policy, "random", seeds)
    report_policies["random"] = {**stats(rand_results, "return"),
                                 **stats(rand_results, "placement"),
                                 "per_seed": rand_results}
    print(f"       done. mean return={report_policies['random']['mean_return']}")

    # ---- RuleTeacher ----
    print(f"[eval] Running RuleTeacher over {len(seeds)} seeds ...")
    rule_results = evaluate_policy(env, rule_teacher_action, "rule_teacher", seeds)
    report_policies["rule_teacher"] = {**stats(rule_results, "return"),
                                        **stats(rule_results, "placement"),
                                        "per_seed": rule_results}
    print(f"       done. mean return={report_policies['rule_teacher']['mean_return']}")

    # ---- PPO (optional) ----
    ppo_policy_fn = None
    if args.ppo_model and os.path.isfile(args.ppo_model):
        ppo_policy_fn = make_ppo_policy(args.ppo_model)
    elif args.ppo_model:
        print(f"[WARN] PPO model not found at {args.ppo_model}, skipping PPO eval.")

    if ppo_policy_fn is not None:
        print(f"[eval] Running PPO over {len(seeds)} seeds ...")
        ppo_results = evaluate_policy(env, ppo_policy_fn, "ppo", seeds)
        report_policies["ppo"] = {**stats(ppo_results, "return"),
                                   **stats(ppo_results, "placement"),
                                   "per_seed": ppo_results}
        print(f"       done. mean return={report_policies['ppo']['mean_return']}")

    env.close()

    # ---- Build comparison ----
    comparison = {}
    if "ppo" in report_policies and "rule_teacher" in report_policies:
        ppo_ret = report_policies["ppo"]["mean_return"]
        ppo_plc = report_policies["ppo"]["mean_placement"]
        rule_ret = report_policies["rule_teacher"]["mean_return"]
        rule_plc = report_policies["rule_teacher"]["mean_placement"]
        comparison["ppo_vs_rule_teacher"] = {
            "return_delta": round(ppo_ret - rule_ret, 4),
            "placement_delta": round(ppo_plc - rule_plc, 4),
            "ppo_better": ppo_ret > rule_ret,
        }

    if "rule_teacher" in report_policies and "random" in report_policies:
        rule_ret = report_policies["rule_teacher"]["mean_return"]
        rule_plc = report_policies["rule_teacher"]["mean_placement"]
        rand_ret = report_policies["random"]["mean_return"]
        rand_plc = report_policies["random"]["mean_placement"]
        comparison["rule_teacher_vs_random"] = {
            "return_delta": round(rule_ret - rand_ret, 4),
            "placement_delta": round(rule_plc - rand_plc, 4),
            "teacher_better": rule_ret > rand_ret,
        }

    # ---- Build report ----
    report = {
        "version": "m1-eval-v1",
        "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "config": {
            "seeds": seeds,
            "max_rounds": args.max_rounds,
        },
        "policies": report_policies,
        "comparison": comparison,
    }

    # ---- Write JSON ----
    out_path = Path(args.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w") as f:
        json.dump(report, f, indent=2)
    print(f"\n[eval] Report written to {out_path}")

    # ---- Print summary table ----
    print("\n=== M1 Eval Report ===")
    hdr = f"{'Policy':<15} {'Mean Return':>12} {'Std':>6} {'Mean Place':>11} {'Std':>6}"
    print(hdr)
    print("-" * len(hdr))

    ordered = ["random", "rule_teacher", "ppo"]
    for name in ordered:
        if name not in report_policies:
            continue
        p = report_policies[name]
        print(f"{name:<15} {p['mean_return']:>12.2f} {p['std_return']:>6.2f} "
              f"{p['mean_placement']:>11.2f} {p['std_placement']:>6.2f}")

    print()

    if "ppo_vs_rule_teacher" in comparison:
        c = comparison["ppo_vs_rule_teacher"]
        verdict = "PPO BETTER" if c["ppo_better"] else "RULE BETTER"
        print(f"PPO vs RuleTeacher: return {c['return_delta']:+.2f}, "
              f"placement {c['placement_delta']:+.2f} => {verdict}")

    if "rule_teacher_vs_random" in comparison:
        c = comparison["rule_teacher_vs_random"]
        verdict = "TEACHER BETTER" if c["teacher_better"] else "RANDOM BETTER"
        print(f"RuleTeacher vs Random: return {c['return_delta']:+.2f}, "
              f"placement {c['placement_delta']:+.2f} => {verdict}")

    # ---- Final verdict ----
    print()
    ppo_beats_rule = comparison.get("ppo_vs_rule_teacher", {}).get("ppo_better", False)
    rule_beats_rand = comparison.get("rule_teacher_vs_random", {}).get("teacher_better", False)

    if ppo_beats_rule and rule_beats_rand:
        print("M1 VERDICT: PASS (PPO > RuleTeacher > Random)")
    elif rule_beats_rand:
        print("M1 VERDICT: PARTIAL (RuleTeacher > Random, but PPO not best or missing)")
    else:
        print("M1 VERDICT: FAIL (RuleTeacher did not beat Random)")


if __name__ == "__main__":
    main()
