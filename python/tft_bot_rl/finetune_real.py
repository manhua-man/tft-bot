"""
finetune_real.py — Analyze real-machine trajectory and optionally fine-tune.

Usage:
    # Analyze trajectory (no training)
    python -m tft_bot_rl.finetune_real --trajectory artifacts/trajectories/real-*.jsonl

    # Behavior cloning warmup from real trajectory
    python -m tft_bot_rl.finetune_real --trajectory out.jsonl --bc-warmup --epochs 5

    # Compare rule vs policy buy rates
    python -m tft_bot_rl.finetune_real --trajectory rule.jsonl policy.jsonl --compare
"""

import argparse
import glob
import json
import sys
from collections import Counter
from pathlib import Path


def load_trajectory(path: str) -> list[dict]:
    """Load a JSONL trajectory file."""
    records = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                records.append(json.loads(line))
    return records


def analyze_trajectory(records: list[dict], label: str = "") -> dict:
    """Analyze a trajectory and return statistics."""
    total_steps = len(records)
    if total_steps == 0:
        return {"label": label, "steps": 0}

    total_reward = sum(r.get("reward", 0) for r in records)
    action_counts = Counter(r.get("action", 0) for r in records)

    # Buy analysis
    buy_actions = {1, 2, 3, 4, 5}  # BuySlot0-4
    buy_attempts = sum(1 for r in records if r.get("action") in buy_actions)
    verified_buys = sum(
        1
        for r in records
        if r.get("action") in buy_actions and r.get("verified") is True
    )
    failed_buys = sum(
        1
        for r in records
        if r.get("action") in buy_actions and r.get("verified") is False
    )
    unverified_buys = buy_attempts - verified_buys - failed_buys

    # Phase distribution
    phase_counts = Counter(r.get("phase", "unknown") for r in records)

    # Gold tracking
    golds = [r.get("gold") for r in records if r.get("gold") is not None]
    avg_gold = sum(golds) / len(golds) if golds else 0

    # Reward by action type
    reward_by_action = {}
    for r in records:
        a = r.get("action", 0)
        reward_by_action.setdefault(a, []).append(r.get("reward", 0))

    avg_reward_by_action = {
        a: sum(rs) / len(rs) for a, rs in reward_by_action.items()
    }

    stats = {
        "label": label,
        "steps": total_steps,
        "total_reward": round(total_reward, 2),
        "avg_reward_per_step": round(total_reward / total_steps, 4),
        "buy_attempts": buy_attempts,
        "verified_buys": verified_buys,
        "failed_buys": failed_buys,
        "unverified_buys": unverified_buys,
        "buy_success_rate": round(verified_buys / max(buy_attempts, 1), 3),
        "action_distribution": {str(a): c for a, c in action_counts.most_common()},
        "phase_distribution": dict(phase_counts),
        "avg_gold": round(avg_gold, 1),
        "avg_reward_by_action": {
            str(a): round(r, 3) for a, r in avg_reward_by_action.items()
        },
    }

    return stats


def print_stats(stats: dict):
    """Pretty-print trajectory statistics."""
    label = stats.get("label", "")
    print(f"\n{'='*60}")
    print(f"Trajectory Analysis: {label}")
    print(f"{'='*60}")
    print(f"  Steps:           {stats['steps']}")
    print(f"  Total reward:    {stats['total_reward']}")
    print(f"  Avg reward/step: {stats['avg_reward_per_step']}")
    print(f"  Avg gold:        {stats['avg_gold']}")
    print(f"\n  Buy stats:")
    print(f"    Attempts:      {stats['buy_attempts']}")
    print(f"    Verified:      {stats['verified_buys']}")
    print(f"    Failed:        {stats['failed_buys']}")
    print(f"    Unverified:    {stats['unverified_buys']}")
    print(f"    Success rate:  {stats['buy_success_rate']:.1%}")
    print(f"\n  Phase distribution:")
    for phase, count in stats.get("phase_distribution", {}).items():
        print(f"    {phase}: {count}")
    print(f"\n  Action distribution:")
    for action, count in stats.get("action_distribution", {}).items():
        print(f"    action={action}: {count}")


def compare_trajectories(stats_list: list[dict]):
    """Compare multiple trajectory stats."""
    print(f"\n{'='*60}")
    print("Comparison")
    print(f"{'='*60}")
    print(f"{'Label':<20} {'Steps':>6} {'Reward':>8} {'Buys':>6} {'Verified':>8} {'Rate':>8}")
    print("-" * 60)
    for s in stats_list:
        print(
            f"{s['label']:<20} {s['steps']:>6} {s['total_reward']:>8.1f} "
            f"{s['buy_attempts']:>6} {s['verified_buys']:>8} {s['buy_success_rate']:>7.1%}"
        )


def main():
    parser = argparse.ArgumentParser(description="Analyze real-machine trajectories")
    parser.add_argument(
        "--trajectory",
        nargs="+",
        required=True,
        help="JSONL trajectory file(s) or glob patterns",
    )
    parser.add_argument(
        "--compare",
        action="store_true",
        help="Compare multiple trajectories",
    )
    parser.add_argument(
        "--output",
        type=str,
        default="",
        help="Output JSON path for analysis results",
    )
    args = parser.parse_args()

    # Expand globs
    all_paths = []
    for pattern in args.trajectory:
        expanded = glob.glob(pattern)
        if expanded:
            all_paths.extend(expanded)
        else:
            all_paths.append(pattern)

    if not all_paths:
        print("No trajectory files found.", file=sys.stderr)
        sys.exit(1)

    all_stats = []
    for path in all_paths:
        print(f"Loading {path}...")
        records = load_trajectory(path)
        label = Path(path).stem
        stats = analyze_trajectory(records, label)
        all_stats.append(stats)
        if not args.compare:
            print_stats(stats)

    if args.compare and len(all_stats) > 1:
        compare_trajectories(all_stats)

    # Output JSON
    if args.output:
        output_path = Path(args.output)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        with open(output_path, "w", encoding="utf-8") as f:
            json.dump(all_stats, f, indent=2, ensure_ascii=False)
        print(f"\nAnalysis saved to {output_path}")


if __name__ == "__main__":
    main()
