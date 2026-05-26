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

    /// Get argmax action from logits
    pub fn predict_action(&mut self, obs: &Obs) -> Result<u16> {
        let logits = self.predict_logits(obs)?;
        let best = logits
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i as u16)
            .unwrap_or(0);
        Ok(best)
    }
}
