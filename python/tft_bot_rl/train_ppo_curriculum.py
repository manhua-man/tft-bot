"""PPO training with curriculum learning for TFT SimEnv (35-action space).

Curriculum phases progressively unlock action categories:
  shop_only     (7 actions):  Noop, BuySlot0..4, Reroll
  shop_economy  (16 actions): + BuyXp, LevelUp, PromoteBestBench,
                               PromoteBenchSlot0..4, ChooseAugment0/1/2
  full          (35 actions): all actions

Usage:
  python train_ppo_curriculum.py --curriculum shop_economy --total-steps 500000
  python train_ppo_curriculum.py --curriculum full --smoke
"""

import argparse
import os
import sys


# ---------------------------------------------------------------------------
# Curriculum phase definitions
# ---------------------------------------------------------------------------

CURRICULUM_PHASES = {
    "shop_only": {
        "num_actions": 7,
        "description": "Shop only: Noop, BuySlot0..4, Reroll",
        "phases": [
            {"fraction": 0.0, "actions": 7, "label": "shop_only"},
        ],
    },
    "shop_economy": {
        "num_actions": 16,
        "description": "Shop + economy: adds BuyXp, LevelUp, Promote, Augments",
        "phases": [
            {"fraction": 0.0,  "actions": 7,  "label": "shop_only"},
            {"fraction": 0.3,  "actions": 16, "label": "shop_economy"},
        ],
    },
    "full": {
        "num_actions": 35,
        "description": "Full 35-action space with all game actions",
        "phases": [
            {"fraction": 0.0,  "actions": 7,  "label": "shop_only"},
            {"fraction": 0.25, "actions": 16, "label": "shop_economy"},
            {"fraction": 0.6,  "actions": 35, "label": "full"},
        ],
    },
}


def get_phase_for_progress(curriculum_name, progress):
    """Return (max_actions, phase_label) for the given training progress [0, 1]."""
    phases = CURRICULUM_PHASES[curriculum_name]["phases"]
    current = phases[0]
    for phase in phases:
        if progress >= phase["fraction"]:
            current = phase
        else:
            break
    return current["actions"], current["label"]


def print_curriculum_info(curriculum_name):
    """Print the curriculum phase schedule."""
    cfg = CURRICULUM_PHASES[curriculum_name]
    print(f"\n=== Curriculum: {curriculum_name} ===")
    print(f"  Description: {cfg['description']}")
    print(f"  Total actions at end: {cfg['num_actions']}")
    print(f"  Phase schedule:")
    for i, phase in enumerate(cfg["phases"]):
        pct = phase["fraction"] * 100
        print(f"    [{i}] @{pct:5.1f}% -> {phase['actions']:2d} actions ({phase['label']})")
    print()


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Train PPO on TFT SimEnv with curriculum learning"
    )
    parser.add_argument(
        "--smoke", action="store_true", help="Short smoke test (1000 steps)"
    )
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--total-steps", type=int, default=500_000)
    parser.add_argument("--max-rounds", type=int, default=6)
    parser.add_argument(
        "--agent-cli", type=str, default=None, help="Path to agent-cli binary"
    )
    parser.add_argument(
        "--curriculum",
        type=str,
        choices=["shop_only", "shop_economy", "full"],
        default="shop_economy",
        help="Curriculum mode (default: shop_economy)",
    )
    parser.add_argument(
        "--output", type=str, default="artifacts/ppo_curriculum_tft",
        help="Model save path (without .zip extension)",
    )
    args = parser.parse_args()

    total_steps = 1000 if args.smoke else args.total_steps

    # Late imports to avoid slow --help
    from stable_baselines3 import PPO
    from stable_baselines3.common.vec_env import DummyVecEnv
    from stable_baselines3.common.callbacks import BaseCallback
    from tft_bot_rl.env_client import TftSimEnv

    # Print curriculum info
    print_curriculum_info(args.curriculum)

    def make_env():
        return TftSimEnv(
            agent_cli_path=args.agent_cli,
            seed=args.seed,
            max_rounds=args.max_rounds,
        )

    env = DummyVecEnv([make_env])

    # The underlying env already has Discrete(35) action space.
    # Curriculum progression is tracked via the callback below.
    num_actions = CURRICULUM_PHASES[args.curriculum]["num_actions"]
    print(f"  Environment action space: {env.action_space.n}")
    print(f"  Curriculum final actions: {num_actions}")
    print(f"  Obs shape: {env.observation_space.shape}")

    model = PPO(
        "MlpPolicy",
        env,
        verbose=1,
        learning_rate=3e-4,
        n_steps=128,
        batch_size=64,
        n_epochs=4,
        seed=args.seed,
        tensorboard_log="tensorboard/" if not args.smoke else None,
    )

    # -----------------------------------------------------------------------
    # Curriculum callback: log phase transitions during training
    # -----------------------------------------------------------------------

    class CurriculumCallback(BaseCallback):
        """Logs curriculum phase transitions during training.

        The action masking is handled by the Rust sim itself (it will reject
        invalid actions). This callback tracks and reports phase progress.
        """

        def __init__(self, curriculum_name, total_timesteps, verbose=1):
            super().__init__(verbose)
            self.curriculum_name = curriculum_name
            self.total_timesteps = total_timesteps
            self.current_label = None
            self.current_max_actions = None

        def _on_training_start(self):
            self._update_phase()

        def _on_step(self):
            self._update_phase()
            return True

        def _update_phase(self):
            progress = self.num_timesteps / max(self.total_timesteps, 1)
            max_actions, label = get_phase_for_progress(
                self.curriculum_name, progress
            )
            if label != self.current_label:
                self.current_label = label
                self.current_max_actions = max_actions
                if self.verbose:
                    pct = progress * 100
                    print(
                        f"\n[curriculum] Phase change @{pct:.1f}%: "
                        f"{label} ({max_actions} actions)"
                    )

    callback = CurriculumCallback(
        curriculum_name=args.curriculum,
        total_timesteps=total_steps,
        verbose=1,
    )

    print(f"\nStarting PPO training: {total_steps} steps, seed={args.seed}")
    print(f"Curriculum: {args.curriculum}")

    try:
        model.learn(total_timesteps=total_steps, callback=callback)
    except KeyboardInterrupt:
        print("\nTraining interrupted.")

    # Save
    os.makedirs(os.path.dirname(args.output) or ".", exist_ok=True)
    save_path = args.output if args.output.endswith(".zip") else args.output + ".zip"
    model.save(save_path)
    print(f"\nModel saved to {save_path}")

    # Quick eval
    print("\nRunning quick evaluation...")
    obs = env.reset()
    total_reward = 0.0
    steps = 0
    done = False
    while not done:
        action, _ = model.predict(obs, deterministic=True)
        obs, reward, done, info = env.step(action)
        total_reward += reward[0]
        steps += 1
        if done:
            break
    print(f"Eval: {steps} steps, total_reward={total_reward:.2f}")
    if info and "placement" in info[0]:
        print(f"Placement: {info[0]['placement']}")

    env.close()
    print("Done.")


if __name__ == "__main__":
    main()
