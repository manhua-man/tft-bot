//! Runtime configuration loading for patch-pack data.
//! Replaces compile-time include_str! with runtime file reads.

use std::path::Path;
use std::sync::OnceLock;

use crate::PatchPack;

static RUNTIME_PATCH_PACK: OnceLock<PatchPack> = OnceLock::new();

/// Load patch-pack from a file path at runtime.
/// Returns the loaded PatchPack, or an error if loading fails.
pub fn load_patch_pack(path: &Path) -> Result<&'static PatchPack, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read patch-pack from {}: {}", path.display(), e))?;
    let pack: PatchPack = serde_json::from_str(&raw)
        .map_err(|e| format!("failed to parse patch-pack from {}: {}", path.display(), e))?;
    RUNTIME_PATCH_PACK.set(pack).map_err(|_| "patch-pack already loaded".to_string())?;
    Ok(RUNTIME_PATCH_PACK.get().unwrap())
}

/// Get the runtime-loaded patch-pack, if available.
pub fn get_patch_pack() -> Option<&'static PatchPack> {
    RUNTIME_PATCH_PACK.get()
}

/// Check if a runtime patch-pack has been loaded.
pub fn has_runtime_patch_pack() -> bool {
    RUNTIME_PATCH_PACK.get().is_some()
}
