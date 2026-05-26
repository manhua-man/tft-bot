"""Contract tests for the TftSimEnv gymnasium wrapper.

These tests require agent-cli to be built first.
Run with: pytest tests/test_env_contract.py -v
"""

import pytest
import numpy as np


def test_env_obs_shape():
    """Verify obs from agent-cli has correct shape."""
    from tft_bot_rl.env_client import TftSimEnv

    env = TftSimEnv(seed=0, max_rounds=2)
    obs, info = env.reset()
    assert obs.shape == (34,), f"Expected shape (34,), got {obs.shape}"
    assert np.isfinite(obs).all(), "Obs contains non-finite values"
    env.close()


def test_env_step_returns_valid():
    """Verify step returns valid obs, reward, terminated, truncated."""
    from tft_bot_rl.env_client import TftSimEnv

    env = TftSimEnv(seed=0, max_rounds=2)
    obs, _ = env.reset()
    obs2, reward, terminated, truncated, info = env.step(0)  # Noop
    assert obs2.shape == (34,)
    assert isinstance(reward, float)
    assert isinstance(terminated, bool)
    assert isinstance(truncated, bool)
    env.close()
