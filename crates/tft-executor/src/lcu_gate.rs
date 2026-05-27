//! LCU (League Client Update) gate — reads lockfile and queries game phase.
//!
//! This module provides the same lockfile/API logic as `apps/lcu-probe`,
//! but as a library callable from executor-probe and real_env.
//!
//! When LCU is available, phase detection uses the LCU API.
//! When LCU is unavailable (lockfile empty/missing), callers should
//! fall back to visual (OCR/template) phase detection.

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::time::Duration;

/// Default lockfile path (国服 League Client 安装目录).
pub const DEFAULT_LOCKFILE_PATH: &str = r"G:\wegameapps\英雄联盟\LeagueClient\lockfile";

/// Parsed lockfile content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    pub name: String,
    pub pid: u32,
    pub port: u16,
    pub password: String,
    pub protocol: String,
}

impl Lockfile {
    /// Parse a lockfile line: `name:pid:port:password:protocol`
    pub fn parse(content: &str) -> Result<Self> {
        let line = content.trim();
        if line.is_empty() {
            anyhow::bail!("lockfile is empty — is the client running?");
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 5 {
            anyhow::bail!(
                "lockfile has {} fields, expected 5",
                parts.len()
            );
        }
        Ok(Self {
            name: parts[0].to_string(),
            pid: parts[1].parse().context("parsing lockfile pid")?,
            port: parts[2].parse().context("parsing lockfile port")?,
            password: parts[3].to_string(),
            protocol: parts[4].to_string(),
        })
    }

    pub fn base_url(&self) -> String {
        format!("https://127.0.0.1:{}", self.port)
    }

    pub fn auth_header(&self) -> String {
        let creds = format!("riot:{}", self.password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(creds);
        format!("Basic {}", encoded)
    }
}

/// Read and parse the lockfile from disk.
pub fn read_lockfile(path: &str) -> Result<Lockfile> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("reading lockfile at {}", path))?;
    Lockfile::parse(&content)
}

/// Resolve lockfile path: env override, saved auth file, default CN path, then process search.
pub fn resolve_lockfile_path() -> String {
    if let Ok(p) = std::env::var("LCU_LOCKFILE") {
        if !p.trim().is_empty() {
            return p;
        }
    }
    if read_lockfile(DEFAULT_LOCKFILE_PATH).is_ok() {
        return DEFAULT_LOCKFILE_PATH.to_string();
    }
    if let Some(p) = lockfile_beside_running_client() {
        return p;
    }
    DEFAULT_LOCKFILE_PATH.to_string()
}

/// Resolve the artifacts directory path.
///
/// Uses TFT_REPO_ROOT env var if set, otherwise uses the current working directory.
fn artifacts_dir() -> std::path::PathBuf {
    if let Ok(root) = std::env::var("TFT_REPO_ROOT") {
        std::path::PathBuf::from(root).join("artifacts")
    } else {
        std::path::PathBuf::from("artifacts")
    }
}

/// Try to get LCU connection info from saved auth file (created by extract_lcu_auth.py).
///
/// Returns Some((port, token)) if the file exists and is valid.
pub fn read_saved_auth() -> Option<(u16, String)> {
    let auth_path = artifacts_dir().join("lcu-auth.json");
    if !auth_path.exists() {
        // Try to extract from log files automatically
        extract_auth_from_logs(&auth_path)?;
    }
    let content = std::fs::read_to_string(auth_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let port = json.get("port")?.as_u64()? as u16;
    let token = json.get("token")?.as_str()?.to_string();
    Some((port, token))
}

/// Default log directory for 国服 League Client.
const DEFAULT_LCU_LOG_DIR: &str = r"G:\WeGameApps\英雄联盟\Game\Logs\LeagueClient Logs";

/// Extract LCU auth from LeagueClientUx log files.
///
/// Searches the Game/Logs directory for the most recent LeagueClientUx.log
/// and extracts --app-port and --remoting-auth-token from the command line.
fn extract_auth_from_logs(output_path: &std::path::Path) -> Option<()> {
    let log_dir_str = std::env::var("LCU_LOG_DIR").unwrap_or_else(|_| DEFAULT_LCU_LOG_DIR.to_string());
    let log_dir = std::path::Path::new(&log_dir_str);
    if !log_dir.exists() {
        return None;
    }

    // Find the most recent LeagueClientUx.log
    let mut entries: Vec<_> = std::fs::read_dir(log_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .contains("LeagueClientUx.log")
        })
        .collect();

    entries.sort_by(|a, b| {
        b.file_name()
            .to_string_lossy()
            .cmp(&a.file_name().to_string_lossy())
    });

    let latest = entries.first()?;
    let content = std::fs::read_to_string(latest.path()).ok()?;

    // Extract --app-port and --remoting-auth-token
    let port_re = regex_lite::Regex::new(r"--app-port=(\d+)").ok()?;
    let token_re = regex_lite::Regex::new(r"--remoting-auth-token=([^\s]+)").ok()?;

    let port = port_re.captures(&content)?.get(1)?.as_str().parse::<u16>().ok()?;
    let token = token_re.captures(&content)?.get(1)?.as_str().to_string();

    // Save to file
    let json = serde_json::json!({"port": port, "token": token});
    if let Some(parent) = output_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(output_path, serde_json::to_string(&json).ok()?).ok()?;
    eprintln!("[lcu] Extracted auth from logs: port={port}");
    Some(())
}

fn lockfile_beside_running_client() -> Option<String> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-Process LeagueClient,LeagueClientUx -ErrorAction SilentlyContinue | \
             Select-Object -First 1 -ExpandProperty Path)",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let exe = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if exe.is_empty() {
        return None;
    }
    let path = std::path::Path::new(&exe);
    let dir = path.parent()?.parent()?; // .../LeagueClient/ -> LeagueClient dir
    let lockfile = dir.join("lockfile");
    if lockfile.exists() {
        let content = fs::read_to_string(&lockfile).ok()?;
        if Lockfile::parse(&content).is_ok() {
            return Some(lockfile.to_string_lossy().to_string());
        }
    }
    // LeagueClientUx.exe lives in LeagueClient subfolder on CN installs
    if let Some(parent) = path.parent() {
        let lockfile = parent.join("lockfile");
        if let Some(s) = lockfile.to_str() {
            if lockfile.exists() && read_lockfile(s).is_ok() {
                return Some(lockfile.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Game phase as reported by LCU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GamePhase {
    None,
    Lobby,
    Matchmaking,
    ReadyCheck,
    ChampSelect,
    GameStart,
    InProgress,
    WaitingForStats,
    EndOfGame,
    Reconnect,
    Unknown(String),
}

impl GamePhase {
    /// Parse from LCU JSON string value.
    pub fn from_lcu_str(s: &str) -> Self {
        match s {
            "None" => Self::None,
            "Lobby" => Self::Lobby,
            "Matchmaking" => Self::Matchmaking,
            "ReadyCheck" => Self::ReadyCheck,
            "ChampSelect" => Self::ChampSelect,
            "GameStart" => Self::GameStart,
            "InProgress" => Self::InProgress,
            "WaitingForStats" => Self::WaitingForStats,
            "EndOfGame" => Self::EndOfGame,
            "Reconnect" => Self::Reconnect,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// Is the player in an active game where shop actions are valid?
    pub fn is_in_game(&self) -> bool {
        matches!(self, Self::InProgress)
    }

    /// Is the player in a non-actionable state?
    pub fn is_idle(&self) -> bool {
        matches!(
            self,
            Self::None | Self::Lobby | Self::Matchmaking | Self::ReadyCheck
        )
    }

    /// Can the agent take shop actions in this phase?
    pub fn can_act(&self) -> bool {
        self.is_in_game()
    }
}

impl std::fmt::Display for GamePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(s) => write!(f, "Unknown({})", s),
            other => write!(f, "{:?}", other),
        }
    }
}

/// Result of an LCU probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LcuProbeResult {
    pub available: bool,
    pub lockfile_path: String,
    pub lockfile: Option<Lockfile>,
    pub phase: Option<GamePhase>,
    pub error: Option<String>,
}

/// Probe LCU availability and current game phase.
///
/// Tries: 1) lockfile, 2) saved auth file (from get-lcu-token.cmd).
/// Returns `available: false` if the client is not running or unreachable.
pub fn probe_lcu(lockfile_path: &str) -> LcuProbeResult {
    // Try lockfile first
    if let Ok(lf) = read_lockfile(lockfile_path) {
        let phase = query_gameflow_phase(&lf);
        let has_phase = phase.is_some();
        return LcuProbeResult {
            available: has_phase,
            lockfile_path: lockfile_path.to_string(),
            lockfile: Some(lf),
            phase,
            error: if !has_phase {
                Some("lockfile read OK but gameflow-phase API unreachable".to_string())
            } else {
                None
            },
        };
    }

    // Fallback: try saved auth file
    if let Some((port, token)) = read_saved_auth() {
        let lf = Lockfile {
            name: "LeagueClient".to_string(),
            pid: 0,
            port,
            password: token,
            protocol: "https".to_string(),
        };
        let phase = query_gameflow_phase(&lf);
        let has_phase = phase.is_some();
        let auth_display = artifacts_dir().join("lcu-auth.json").to_string_lossy().to_string();
        return LcuProbeResult {
            available: has_phase,
            lockfile_path: auth_display,
            lockfile: Some(lf),
            phase,
            error: if !has_phase {
                Some("saved auth found but gameflow-phase API unreachable".to_string())
            } else {
                None
            },
        };
    }

    LcuProbeResult {
        available: false,
        lockfile_path: lockfile_path.to_string(),
        lockfile: None,
        phase: None,
        error: Some("lockfile empty and no saved auth file".to_string()),
    }
}

/// Query the gameflow-phase endpoint. Returns None if unreachable.
fn query_gameflow_phase(lf: &Lockfile) -> Option<GamePhase> {
    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;

    let url = format!("{}/lol-gameflow/v1/gameflow-phase", lf.base_url());
    let resp = client
        .get(&url)
        .header("Authorization", lf.auth_header())
        .send()
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body: serde_json::Value = resp.json().ok()?;
    let phase_str = body.as_str()?;
    Some(GamePhase::from_lcu_str(phase_str))
}

/// Gate that enforces phase-based action restrictions.
///
/// If LCU is available, only allows actions when `phase == InProgress`.
/// If LCU is unavailable, always allows actions (caller uses visual detection).
pub struct LcuGate {
    probe: LcuProbeResult,
}

impl LcuGate {
    /// Create a gate by probing LCU.
    pub fn probe(lockfile_path: &str) -> Self {
        Self {
            probe: probe_lcu(lockfile_path),
        }
    }

    /// Create a gate from a known probe result (for testing).
    pub fn from_probe(probe: LcuProbeResult) -> Self {
        Self { probe }
    }

    /// Is LCU available?
    pub fn is_available(&self) -> bool {
        self.probe.available
    }

    /// Current game phase (None if LCU unavailable).
    pub fn phase(&self) -> Option<&GamePhase> {
        self.probe.phase.as_ref()
    }

    /// Can the agent take actions right now?
    ///
    /// Returns `Ok(true)` if actions are allowed.
    /// Returns `Ok(false)` if LCU says we're not in-game (with reason).
    /// Returns `Err` if LCU is unavailable AND the caller requires it.
    pub fn check_can_act(&self, require_lcu: bool) -> Result<ActPermission> {
        if !self.probe.available {
            if require_lcu {
                anyhow::bail!(
                    "LCU not available and required: {}",
                    self.probe.error.as_deref().unwrap_or("unknown")
                );
            }
            // LCU unavailable, but not required — allow with visual fallback
            return Ok(ActPermission {
                allowed: true,
                reason: "LCU unavailable, using visual fallback".to_string(),
                phase: None,
            });
        }

        let phase = self.probe.phase.as_ref().unwrap();
        let allowed = phase.can_act();
        let reason = if allowed {
            format!("LCU phase: {} (can act)", phase)
        } else {
            format!("LCU phase: {} (cannot act)", phase)
        };

        Ok(ActPermission {
            allowed,
            reason,
            phase: Some(phase.clone()),
        })
    }

    /// Get the raw probe result for logging/diagnostics.
    pub fn probe_result(&self) -> &LcuProbeResult {
        &self.probe
    }
}

/// Result of a phase gate check.
#[derive(Debug, Clone)]
pub struct ActPermission {
    pub allowed: bool,
    pub reason: String,
    pub phase: Option<GamePhase>,
}

/// Meta mode for the lobby→game FSM.
///
/// - `Manual` (default): user queues; bot uses 2999 + window
/// - `Lcu`: lobby FSM via LCU (requires readable lockfile)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaMode {
    Lcu,
    Manual,
}

impl MetaMode {
    /// `TFT_META_MODE`: `lcu` → Lcu; `manual` → Manual; unset → auto-detect.
    ///
    /// When unset, probes LCU: if available → Lcu, otherwise → Manual.
    pub fn from_env() -> Self {
        match std::env::var("TFT_META_MODE").as_deref() {
            Ok("lcu") => Self::Lcu,
            Ok("manual") => Self::Manual,
            _ => {
                // Auto-detect: try LCU probe
                let lockfile = resolve_lockfile_path();
                let probe = probe_lcu(&lockfile);
                if probe.available {
                    eprintln!("[meta] Auto-detected LCU available, using Lcu mode");
                    Self::Lcu
                } else {
                    eprintln!("[meta] LCU not available, using Manual mode");
                    Self::Manual
                }
            }
        }
    }
}

impl std::fmt::Display for MetaMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lcu => write!(f, "lcu"),
            Self::Manual => write!(f, "manual"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lockfile() {
        let lf = Lockfile::parse("LeagueClient:12345:54321:abcdef:https").unwrap();
        assert_eq!(lf.name, "LeagueClient");
        assert_eq!(lf.pid, 12345);
        assert_eq!(lf.port, 54321);
        assert_eq!(lf.password, "abcdef");
        assert_eq!(lf.protocol, "https");
    }

    #[test]
    fn parse_empty_lockfile_fails() {
        assert!(Lockfile::parse("").is_err());
    }

    #[test]
    fn game_phase_from_str() {
        assert_eq!(GamePhase::from_lcu_str("InProgress"), GamePhase::InProgress);
        assert_eq!(GamePhase::from_lcu_str("Lobby"), GamePhase::Lobby);
        assert!(matches!(
            GamePhase::from_lcu_str("SomethingElse"),
            GamePhase::Unknown(_)
        ));
    }

    #[test]
    fn game_phase_can_act() {
        assert!(GamePhase::InProgress.can_act());
        assert!(!GamePhase::Lobby.can_act());
        assert!(!GamePhase::ChampSelect.can_act());
        assert!(!GamePhase::None.can_act());
    }

    #[test]
    fn gate_allows_when_lcu_unavailable() {
        let probe = LcuProbeResult {
            available: false,
            lockfile_path: "/fake".to_string(),
            lockfile: None,
            phase: None,
            error: Some("lockfile empty".to_string()),
        };
        let gate = LcuGate::from_probe(probe);
        let perm = gate.check_can_act(false).unwrap();
        assert!(perm.allowed);
    }

    #[test]
    fn gate_blocks_when_lcu_unavailable_and_required() {
        let probe = LcuProbeResult {
            available: false,
            lockfile_path: "/fake".to_string(),
            lockfile: None,
            phase: None,
            error: Some("lockfile empty".to_string()),
        };
        let gate = LcuGate::from_probe(probe);
        assert!(gate.check_can_act(true).is_err());
    }

    #[test]
    fn gate_blocks_when_in_lobby() {
        let probe = LcuProbeResult {
            available: true,
            lockfile_path: "/fake".to_string(),
            lockfile: None,
            phase: Some(GamePhase::Lobby),
            error: None,
        };
        let gate = LcuGate::from_probe(probe);
        let perm = gate.check_can_act(false).unwrap();
        assert!(!perm.allowed);
    }

    #[test]
    fn gate_allows_when_in_progress() {
        let probe = LcuProbeResult {
            available: true,
            lockfile_path: "/fake".to_string(),
            lockfile: None,
            phase: Some(GamePhase::InProgress),
            error: None,
        };
        let gate = LcuGate::from_probe(probe);
        let perm = gate.check_can_act(false).unwrap();
        assert!(perm.allowed);
    }
}
