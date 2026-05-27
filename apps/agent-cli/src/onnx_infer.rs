//! ONNX policy inference for agent-cli.

use anyhow::{Context, Result};
use ort::session::Session;
use ort::value::Tensor;
use tft_env::Obs;

pub struct OnnxPolicy {
    session: Session,
}

impl OnnxPolicy {
    pub fn load(path: &str) -> Result<Self> {
        let session = Session::builder()?
            .commit_from_file(path)
            .with_context(|| format!("loading ONNX model from {path}"))?;
        Ok(Self { session })
    }

    /// Run inference: obs -> action_logits (f32 vec of length action_count)
    pub fn predict_logits(&mut self, obs: &Obs) -> Result<Vec<f32>> {
        let obs_vec = obs.to_vec();
        let input =
            Tensor::from_array(([1usize, obs_vec.len()], obs_vec.into_boxed_slice()))
                .map_err(|e| anyhow::anyhow!("tensor creation failed: {e}"))?;
        let outputs = self
            .session
            .run(ort::inputs![input])
            .map_err(|e| anyhow::anyhow!("inference failed: {e}"))?;

        // Extract first output tensor (same API as tft-strategy)
        let (_, logits) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow::anyhow!("output extraction failed: {e}"))?;

        Ok(logits.to_vec())
    }

    /// Get argmax action from logits (no masking).
    pub fn predict_action(&mut self, obs: &Obs) -> Result<u16> {
        let logits = self.predict_logits(obs)?;
        Ok(argmax_f32(&logits))
    }

    /// Get argmax action from logits, with legal_mask applied.
    ///
    /// Illegal actions (mask=false) get -inf before argmax.
    /// This prevents the agent from repeatedly choosing illegal actions.
    pub fn predict_action_masked(&mut self, obs: &Obs, legal_mask: &[bool]) -> Result<u16> {
        let mut logits = self.predict_logits(obs)?;

        // Pad or truncate mask to match logits length
        for (i, logit) in logits.iter_mut().enumerate() {
            if i >= legal_mask.len() || !legal_mask[i] {
                *logit = f32::NEG_INFINITY;
            }
        }

        Ok(argmax_f32(&logits))
    }
}

/// Get the index of the maximum f32 value. NaN-safe via total_cmp.
fn argmax_f32(values: &[f32]) -> u16 {
    values
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(i, _)| i as u16)
        .unwrap_or(0)
}
