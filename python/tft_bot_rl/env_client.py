"""Gymnasium Env wrapper that communicates with agent-cli sim-env via JSON Lines subprocess."""

import gymnasium as gym
import numpy as np
import subprocess
import json
import os
import sys


class TftSimEnv(gym.Env):
    """TFT simulation environment backed by the Rust agent-cli binary.

    Spawns `agent-cli sim-env` as a subprocess and communicates via
    JSON Lines over stdin/stdout.
    """

    metadata = {"render_modes": []}

    def __init__(self, agent_cli_path=None, seed=42, max_rounds=6):
        super().__init__()
        if agent_cli_path is None:
            # Default: look in target/debug relative to project root
            here = os.path.dirname(os.path.abspath(__file__))
            project_root = os.path.join(here, "..", "..")
            exe_name = "agent-cli.exe" if sys.platform == "win32" else "agent-cli"
            agent_cli_path = os.path.join(project_root, "target", "debug", exe_name)
        self.agent_cli_path = agent_cli_path
        self.seed_val = seed
        self.max_rounds = max_rounds
        self.proc = None

        self.obs_dim = 34
        self.action_space = gym.spaces.Discrete(35)
        self.observation_space = gym.spaces.Box(
            low=-np.inf, high=np.inf, shape=(self.obs_dim,), dtype=np.float32
        )

    def _start_proc(self, seed):
        """Kill any existing subprocess and start a fresh agent-cli."""
        if self.proc is not None:
            try:
                self.proc.stdin.close()
                self.proc.terminate()
                self.proc.wait(timeout=5)
            except Exception:
                self.proc.kill()
                self.proc.wait()

        self.proc = subprocess.Popen(
            [
                self.agent_cli_path,
                "sim-env",
                "--seed", str(seed),
                "--max-rounds", str(self.max_rounds),
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )

    def _parse_obs(self, obs_dict):
        """Flatten the obs dict into a single float32 array."""
        return np.array(
            obs_dict["scalars"]
            + obs_dict["shop_costs"]
            + obs_dict["shop_preferred"]
            + obs_dict["board_cost_dist"]
            + obs_dict["phase"]
            + obs_dict["flags"],
            dtype=np.float32,
        )

    def _read_line(self):
        """Read a JSON line from the subprocess stdout, with error handling."""
        line = self.proc.stdout.readline().strip()
        if not line:
            raise RuntimeError("agent-cli produced no output (exited early?)")
        try:
            return json.loads(line)
        except json.JSONDecodeError as e:
            raise RuntimeError(f"Invalid JSON from agent-cli: {line!r}") from e

    def reset(self, seed=None, options=None):
        """Start a new simulation episode."""
        super().reset(seed=seed)
        actual_seed = seed if seed is not None else self.seed_val
        self._start_proc(actual_seed)

        msg = self._read_line()
        # msg should be {"type": "reset", "obs": {...}}
        obs = self._parse_obs(msg["obs"])
        return obs, {}

    def step(self, action):
        """Send an action and receive the next observation."""
        self.proc.stdin.write(f"{action}\n")
        self.proc.stdin.flush()

        msg = self._read_line()
        # msg should be {"type": "step", "result": {...}}
        result = msg["result"]
        obs = self._parse_obs(result["obs"])
        reward = float(result["reward"])
        terminated = bool(result["terminated"])
        truncated = bool(result["truncated"])
        info = result.get("info", {})

        if terminated:
            # Read the outcome line
            try:
                outcome_line = self.proc.stdout.readline().strip()
                if outcome_line:
                    outcome = json.loads(outcome_line)
                    info.update(outcome.get("outcome", {}))
            except (json.JSONDecodeError, RuntimeError):
                pass  # Best-effort outcome parsing

        return obs, reward, terminated, truncated, info

    def close(self):
        """Clean up the subprocess."""
        if self.proc is not None:
            try:
                self.proc.stdin.close()
                self.proc.terminate()
                self.proc.wait(timeout=5)
            except Exception:
                try:
                    self.proc.kill()
                    self.proc.wait()
                except Exception:
                    pass
            self.proc = None
