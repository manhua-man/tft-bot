//! LCU Probe — detect if the League Client (LCU) is accessible on the local machine.
//!
//! Reads the lockfile, connects via HTTPS to 127.0.0.1:{port},
//! queries gameflow-phase and TFT-related endpoints, and outputs JSON.
//!
//! Usage:
//!   lcu-probe                              # probe with default lockfile path
//!   lcu-probe --lockfile <path>            # explicit lockfile path
//!   lcu-probe --accept --i-know            # send ready-check accept (CAUTION)
//!   lcu-probe --accept --dry-run           # show what would be sent (default)

use anyhow::{Context, Result};
use base64::Engine;
use reqwest::blocking::Client;
use serde::Serialize;
use std::env;
use std::fs;
use std::time::Duration;

/// Default lockfile path (国服 League Client)
const DEFAULT_LOCKFILE_PATH: &str = r"G:\wegameapps\英雄联盟\LeagueClient\lockfile";

#[derive(Debug, Clone)]
struct Lockfile {
    name: String,
    pid: u32,
    port: u16,
    password: String,
    protocol: String,
}

impl Lockfile {
    fn parse(content: &str) -> Result<Self> {
        let line = content.trim();
        if line.is_empty() {
            anyhow::bail!("lockfile is empty — is the client running?");
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 5 {
            anyhow::bail!(
                "lockfile has {} fields, expected 5 (name:pid:port:password:protocol)",
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

    fn base_url(&self) -> String {
        format!("https://127.0.0.1:{}", self.port)
    }

    fn auth_header(&self) -> String {
        let creds = format!("riot:{}", self.password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(creds);
        format!("Basic {}", encoded)
    }
}

#[derive(Debug, Serialize)]
struct ProbeResult {
    lockfile_found: bool,
    lockfile_path: String,
    lockfile_path_tried: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lockfile: Option<LockfileInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoints: Option<EndpointResults>,
    #[serde(skip_serializing_if = "Option::is_none")]
    accept_result: Option<AcceptResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    league_process_paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    verdict: String,
}

#[derive(Debug, Serialize)]
struct LockfileInfo {
    name: String,
    pid: u32,
    port: u16,
    protocol: String,
}

#[derive(Debug, Serialize)]
struct EndpointResults {
    gameflow_phase: EndpointResult,
    ready_check: EndpointResult,
    lol_champions: EndpointResult,
    tft_companion: EndpointResult,
}

#[derive(Debug, Serialize)]
struct EndpointResult {
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct AcceptResult {
    attempted: bool,
    dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    ready_check_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    accept_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    accept_body: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Discover lockfile paths to try, in priority order.
fn discover_lockfile_paths(explicit: &str) -> Vec<String> {
    let mut tried = Vec::new();

    // 1. Explicit / env override
    if !explicit.is_empty() {
        tried.push(explicit.to_string());
    }

    // 2. Default CN WeGame path
    if explicit != DEFAULT_LOCKFILE_PATH {
        tried.push(DEFAULT_LOCKFILE_PATH.to_string());
    }

    // 3. Beside running process
    if let Some(p) = lockfile_beside_running_client() {
        if !tried.contains(&p) {
            tried.push(p);
        }
    }

    tried
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
    let dir = path.parent()?.parent()?;
    let lockfile = dir.join("lockfile");
    if lockfile.exists() {
        let content = fs::read_to_string(&lockfile).ok()?;
        if Lockfile::parse(&content).is_ok() {
            return Some(lockfile.to_string_lossy().to_string());
        }
    }
    if let Some(parent) = path.parent() {
        let lockfile = parent.join("lockfile");
        if let Some(s) = lockfile.to_str() {
            if lockfile.exists()
                && fs::read_to_string(s)
                    .map(|c| Lockfile::parse(&c).is_ok())
                    .unwrap_or(false)
            {
                return Some(lockfile.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Find LeagueClient process paths for diagnostics.
fn find_league_process_paths() -> Vec<String> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-Process LeagueClient,LeagueClientUx,\"League of Legends\" \
             -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Path)",
        ])
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        _ => vec![],
    }
}

fn read_lockfile_from_tried(tried: &[String]) -> Option<(Lockfile, String)> {
    for path in tried {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(lf) = Lockfile::parse(&content) {
                return Some((lf, path.clone()));
            }
        }
    }
    None
}

fn build_client() -> Result<Client> {
    Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .no_proxy()
        .build()
        .context("building HTTP client")
}

fn probe_endpoint(client: &Client, lf: &Lockfile, path: &str) -> EndpointResult {
    let url = format!("{}{}", lf.base_url(), path);
    match client
        .get(&url)
        .header("Authorization", lf.auth_header())
        .send()
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.json::<serde_json::Value>().ok();
            EndpointResult {
                status,
                body,
                error: None,
            }
        }
        Err(e) => EndpointResult {
            status: 0,
            body: None,
            error: Some(e.to_string()),
        },
    }
}

fn read_saved_auth() -> Option<(u16, String)> {
    let auth_path = std::path::Path::new("F:/tft-bot/artifacts/lcu-auth.json");
    if !auth_path.exists() {
        extract_auth_from_logs(auth_path)?;
    }
    let content = fs::read_to_string(auth_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let port = json.get("port")?.as_u64()? as u16;
    let token = json.get("token")?.as_str()?.to_string();
    Some((port, token))
}

fn extract_auth_from_logs(output_path: &std::path::Path) -> Option<()> {
    let log_dir = std::path::Path::new(r"G:\WeGameApps\英雄联盟\Game\Logs\LeagueClient Logs");
    if !log_dir.exists() { return None; }
    let mut entries: Vec<_> = fs::read_dir(log_dir).ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("LeagueClientUx.log"))
        .collect();
    entries.sort_by(|a, b| b.file_name().to_string_lossy().cmp(&a.file_name().to_string_lossy()));
    let latest = entries.first()?;
    let content = fs::read_to_string(latest.path()).ok()?;
    let port_start = content.find("--app-port=")? + 11;
    let port_end = content[port_start..].find(|c: char| !c.is_ascii_digit())? + port_start;
    let port: u16 = content[port_start..port_end].parse().ok()?;
    let token_start = content.find("--remoting-auth-token=")? + 22;
    let token_end = content[token_start..].find(|c: char| c.is_whitespace() || c == '"')? + token_start;
    let token = content[token_start..token_end].to_string();
    let json = serde_json::json!({"port": port, "token": token});
    if let Some(parent) = output_path.parent() { let _ = fs::create_dir_all(parent); }
    fs::write(output_path, serde_json::to_string(&json).ok()?).ok()?;
    Some(())
}

fn run_probe(lockfile_path: &str, do_accept: bool, dry_run: bool) -> ProbeResult {
    let tried = discover_lockfile_paths(lockfile_path);
    let league_paths = find_league_process_paths();

    // Try to read lockfile from discovered paths
    let lf = match read_lockfile_from_tried(&tried) {
        Some((lf, _)) => lf,
        None => {
            // Fallback: try saved auth file (from extract_lcu_auth.py or log extraction)
            if let Some((port, token)) = read_saved_auth() {
                Lockfile {
                    name: "LeagueClient".to_string(),
                    pid: 0,
                    port,
                    password: token,
                    protocol: "https".to_string(),
                }
            } else {
                return ProbeResult {
                    lockfile_found: false,
                    lockfile_path: lockfile_path.to_string(),
                    lockfile_path_tried: tried,
                    lockfile: None,
                    endpoints: None,
                    accept_result: None,
                    league_process_paths: if league_paths.is_empty() {
                        None
                    } else {
                        Some(league_paths)
                    },
                    error: Some("lockfile not found and no saved auth".to_string()),
                    verdict: "LCU_NOT_RUNNING".to_string(),
                };
            }
        }
    };

    // Build HTTPS client
    let client = match build_client() {
        Ok(c) => c,
        Err(e) => {
            return ProbeResult {
                lockfile_found: true,
                lockfile_path: lockfile_path.to_string(),
                lockfile_path_tried: tried,
                lockfile: Some(LockfileInfo {
                    name: lf.name.clone(),
                    pid: lf.pid,
                    port: lf.port,
                    protocol: lf.protocol.clone(),
                }),
                endpoints: None,
                accept_result: None,
                league_process_paths: if league_paths.is_empty() {
                    None
                } else {
                    Some(league_paths)
                },
                error: Some(format!("HTTP client build failed: {}", e)),
                verdict: "CLIENT_BUILD_ERROR".to_string(),
            };
        }
    };

    // Probe endpoints
    let gameflow = probe_endpoint(&client, &lf, "/lol-gameflow/v1/gameflow-phase");
    let ready_check = probe_endpoint(&client, &lf, "/lol-matchmaking/v1/ready-check");
    let champions = probe_endpoint(&client, &lf, "/lol-champions/v1/inventories");
    let tft = probe_endpoint(&client, &lf, "/lol-game-queues/v1/queues");

    let endpoints = EndpointResults {
        gameflow_phase: gameflow,
        ready_check,
        lol_champions: champions,
        tft_companion: tft,
    };

    // Accept probe
    let accept_result = if do_accept {
        let rc_state = endpoints
            .ready_check
            .body
            .as_ref()
            .and_then(|b| b.get("state"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if dry_run {
            AcceptResult {
                attempted: true,
                dry_run: true,
                ready_check_state: rc_state,
                accept_status: None,
                accept_body: None,
                error: None,
            }
        } else {
            // Actually send accept
            let url = format!("{}/lol-matchmaking/v1/ready-check/accept", lf.base_url());
            match client
                .post(&url)
                .header("Authorization", lf.auth_header())
                .send()
            {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let body = resp.json::<serde_json::Value>().ok();
                    AcceptResult {
                        attempted: true,
                        dry_run: false,
                        ready_check_state: rc_state,
                        accept_status: Some(status),
                        accept_body: body,
                        error: None,
                    }
                }
                Err(e) => AcceptResult {
                    attempted: true,
                    dry_run: false,
                    ready_check_state: rc_state,
                    accept_status: None,
                    accept_body: None,
                    error: Some(e.to_string()),
                },
            }
        }
    } else {
        AcceptResult {
            attempted: false,
            dry_run: false,
            ready_check_state: None,
            accept_status: None,
            accept_body: None,
            error: None,
        }
    };

    // Determine verdict
    let verdict = if endpoints.gameflow_phase.status == 200 {
        let phase = endpoints
            .gameflow_phase
            .body
            .as_ref()
            .and_then(|b| b.as_str())
            .unwrap_or("Unknown");
        format!("LCU_OK (phase={})", phase)
    } else if endpoints.gameflow_phase.status == 404 {
        "LCU_OK_BUT_NO_GAMEFLOW_ENDPOINT".to_string()
    } else if endpoints.gameflow_phase.status == 0 {
        "LCU_UNREACHABLE".to_string()
    } else {
        format!("LCU_HTTP_{}", endpoints.gameflow_phase.status)
    };

    ProbeResult {
        lockfile_found: true,
        lockfile_path: lockfile_path.to_string(),
        lockfile_path_tried: tried,
        lockfile: Some(LockfileInfo {
            name: lf.name,
            pid: lf.pid,
            port: lf.port,
            protocol: lf.protocol,
        }),
        endpoints: Some(endpoints),
        accept_result: Some(accept_result),
        league_process_paths: if league_paths.is_empty() { None } else { Some(league_paths) },
        error: None,
        verdict,
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut lockfile_path = String::new();
    let mut do_accept = false;
    let mut dry_run = true;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--lockfile" => {
                i += 1;
                if i < args.len() {
                    lockfile_path = args[i].clone();
                }
            }
            "--accept" => {
                do_accept = true;
            }
            "--i-know" => {
                dry_run = false;
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--help" | "-h" => {
                eprintln!("Usage: lcu-probe [OPTIONS]");
                eprintln!();
                eprintln!("Probes the local League Client (LCU) for accessibility.");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  --lockfile <path>  Explicit lockfile path");
                eprintln!("  --accept           Probe ready-check accept");
                eprintln!("  --i-know           Actually send accept (not just dry-run)");
                eprintln!("  --dry-run          Show what would be sent (default with --accept)");
                eprintln!();
                eprintln!("Environment variables:");
                eprintln!("  LCU_LOCKFILE  Override default lockfile path");
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {}", other);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // Allow env override
    if lockfile_path.is_empty() {
        if let Ok(env_path) = env::var("LCU_LOCKFILE") {
            lockfile_path = env_path;
        }
    }
    if lockfile_path.is_empty() {
        lockfile_path = DEFAULT_LOCKFILE_PATH.to_string();
    }

    let result = run_probe(&lockfile_path, do_accept, dry_run);

    // Always output JSON
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );

    // Exit code: 0 if LCU_OK*, 1 otherwise
    if result.verdict.starts_with("LCU_OK") {
        std::process::exit(0);
    } else {
        std::process::exit(1);
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
        assert_eq!(lf.base_url(), "https://127.0.0.1:54321");
    }

    #[test]
    fn parse_empty_lockfile_fails() {
        assert!(Lockfile::parse("").is_err());
    }

    #[test]
    fn parse_short_lockfile_fails() {
        assert!(Lockfile::parse("a:b:c").is_err());
    }

    #[test]
    fn auth_header_format() {
        let lf = Lockfile::parse("LeagueClient:1:5000:testpw:https").unwrap();
        let auth = lf.auth_header();
        assert!(auth.starts_with("Basic "));
    }
}
