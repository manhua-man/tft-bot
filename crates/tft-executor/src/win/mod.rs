//! Windows-specific executor implementations.
//!
//! Only compiled on Windows with the `win_window` feature.

#[cfg(feature = "win_window")]
pub mod window_discovery;
