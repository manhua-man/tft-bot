"""PPO training entry point for TFT SimEnv using Stable Baselines 3."""

import argparse
import os
import sys


def main():
    parser = argparse.ArgumentParser(description="Train PPO on TFT SimEnv")
    parser.add_argument(
        "--smoke", action="store_true", help="Short smoke test (1000 steps)"
    )
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--total-steps", type=int, default=100_000)
    parser.add_argument("--max-rounds", type=int, default=6)
    parser.add_argument(
        "--agent-cli", type=str, default=None, help="Path to agent-cli binary"
    )
    args = parser.parse_args()

    total_steps = 1000 if args.smoke else args.total_steps

    from stable_baselines3 import PPO
    from stable_baselines3.common.vec_env import DummyVecEnv
    from tft_bot_rl.env_client import TftSimEnv

    def make_env():
        return TftSimEnv(
            agent_cli_path=args.agent_cli,
            seed=args.seed,
            max_rounds=args.max_rounds,
        )

    env = DummyVecEnv([make_env])

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

    print(f"Starting PPO training: {total_steps} steps, seed={args.seed}")
    print(f"Obs dim: {env.observation_space.shape}, Actions: {env.action_space.n}")

    try:
        model.learn(total_timesteps=total_steps)
    except KeyboardInterrupt:
        print("\nTraining interrupted.")

    # Save
    os.makedirs("artifacts", exist_ok=True)
    model_path = "artifacts/ppo_tft.zip"
    model.save(model_path)
    print(f"Model saved to {model_path}")

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
