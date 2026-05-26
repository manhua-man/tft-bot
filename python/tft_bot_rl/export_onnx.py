"""Export trained PPO model to ONNX for Rust inference."""

import argparse
import os
import torch
import numpy as np

def main():
    parser = argparse.ArgumentParser(description="Export PPO model to ONNX")
    parser.add_argument("--model", type=str, required=True, help="Path to trained PPO .zip")
    parser.add_argument("--output", type=str, default="artifacts/ppo_tft.onnx", help="Output ONNX path")
    parser.add_argument("--obs-dim", type=int, default=34)
    parser.add_argument("--action-dim", type=int, default=15)
    args = parser.parse_args()

    from stable_baselines3 import PPO

    print(f"Loading model from {args.model}...")
    model = PPO.load(args.model)

    # Get the policy network
    policy = model.policy
    policy.eval()

    # Create dummy input
    dummy_input = torch.randn(1, args.obs_dim)

    # Export
    os.makedirs(os.path.dirname(args.output) or ".", exist_ok=True)

    # For SB3 MlpPolicy, we need to extract just the forward pass
    # that maps obs -> action_logits
    class PolicyWrapper(torch.nn.Module):
        def __init__(self, policy):
            super().__init__()
            self.policy = policy
        
        def forward(self, x):
            # Extract action distribution logits
            features = self.policy.extract_features(x)
            latent_pi, latent_vf = self.policy.mlp_extractor(features)
            action_logits = self.policy.action_net(latent_pi)
            return action_logits

    wrapper = PolicyWrapper(policy)
    wrapper.eval()

    print(f"Exporting to {args.output}...")
    torch.onnx.export(
        wrapper,
        dummy_input,
        args.output,
        input_names=["observation"],
        output_names=["action_logits"],
        dynamic_axes={
            "observation": {0: "batch_size"},
            "action_logits": {0: "batch_size"},
        },
        opset_version=17,
    )
    print(f"Exported to {args.output}")
    print(f"  Input: observation [{args.obs_dim}]")
    print(f"  Output: action_logits [{args.action_dim}]")


if __name__ == "__main__":
    main()
