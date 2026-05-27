//! Real window discovery using Win32 `EnumWindows`.
//!
//! Matches game windows by title regex from `window_profiles.cn.yaml`.
//! Only compiled when `win_window` feature is enabled.

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC, HDC, LOGPIXELSX};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowInfo, GetWindowTextW, IsIconic, IsWindowVisible, WINDOWINFO,
    WS_EX_TOOLWINDOW,
};

use crate::window::GameWindow;
use crate::window::WindowDiscovery;

/// Configuration for matching game windows.
#[derive(Debug, Clone)]
pub struct WindowMatchConfig {
    /// Window title patterns (substring match, case-insensitive)
    pub title_patterns: Vec<String>,
    /// Process names to match (for future extension)
    pub process_names: Vec<String>,
    /// Minimum window dimensions to consider
    pub min_width: u32,
    pub min_height: u32,
}

impl Default for WindowMatchConfig {
    fn default() -> Self {
        Self {
            title_patterns: vec![
                "英雄联盟".to_string(),
                "League of Legends".to_string(),
                "云顶之弈".to_string(),
                "Teamfight Tactics".to_string(),
                "TFT".to_string(),
            ],
            process_names: vec![
                "League of Legends.exe".to_string(),
                "LeagueClientUx.exe".to_string(),
            ],
            min_width: 800,
            min_height: 600,
        }
    }
}

/// Real window discovery using Win32 API.
pub struct WinWindowDiscovery {
    config: WindowMatchConfig,
}

impl WinWindowDiscovery {
    pub fn new(config: WindowMatchConfig) -> Self {
        Self { config }
    }

    pub fn with_defaults() -> Self {
        Self::new(WindowMatchConfig::default())
    }
}

impl WindowDiscovery for WinWindowDiscovery {
    fn find_game_window(&self) -> Result<GameWindow> {
        let candidates = Arc::new(Mutex::new(Vec::<GameWindow>::new()));
        let config = self.config.clone();

        // Leak one Arc reference into the raw pointer for the callback.
        let raw_ptr = Arc::into_raw(candidates.clone()) as isize;

        unsafe {
            let _ = EnumWindows(
                Some(enum_windows_callback),
                LPARAM(raw_ptr),
            );
            // Reclaim the leaked Arc so the refcount is correct.
            let _ = Arc::from_raw(raw_ptr as *const Mutex<Vec<GameWindow>>);
        }

        let windows = Arc::try_unwrap(candidates)
            .map_err(|_| anyhow::anyhow!("Arc still shared"))?
            .into_inner()
            .map_err(|e| anyhow::anyhow!("mutex poisoned: {}", e))?;

        // Filter by config
        let matched: Vec<GameWindow> = windows
            .into_iter()
            .filter(|w| {
                if w.width < config.min_width || w.height < config.min_height {
                    return false;
                }
                let title_lower = w.title.to_lowercase();
                config
                    .title_patterns
                    .iter()
                    .any(|pat| title_lower.contains(&pat.to_lowercase()))
            })
            .collect();

        if matched.is_empty() {
            if let Ok(w) = find_via_process_main_window(&config) {
                return Ok(w);
            }
            anyhow::bail!(
                "No game window found matching patterns: {:?}",
                config.title_patterns
            );
        }

        // Prefer largest visible window
        let best = matched.iter().max_by_key(|w| w.width * w.height).unwrap();
        Ok(best.clone())
    }
}

/// Fallback: League Client Ux often omits itself from naive EnumWindows filters.
fn find_via_process_main_window(config: &WindowMatchConfig) -> Result<GameWindow> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "$p = Get-Process LeagueClientUx,LeagueClient,'League of Legends' -ErrorAction SilentlyContinue | \
             Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1; \
             if ($p) { \"$($p.MainWindowHandle)|$($p.MainWindowTitle)\" }",
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("powershell process window query failed");
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if line.is_empty() || !line.contains('|') {
        anyhow::bail!("no process main window");
    }
    let (handle_str, title) = line
        .split_once('|')
        .ok_or_else(|| anyhow::anyhow!("bad powershell output"))?;
    let handle: isize = handle_str.trim().parse().context("hwnd parse")?;
    if handle == 0 {
        anyhow::bail!("null hwnd");
    }
    let title_lower = title.to_lowercase();
    if !config
        .title_patterns
        .iter()
        .any(|pat| title_lower.contains(&pat.to_lowercase()))
    {
        anyhow::bail!("process window title does not match: {}", title);
    }
    game_window_from_hwnd(HWND(handle as *mut _))
}

fn game_window_from_hwnd(hwnd: HWND) -> Result<GameWindow> {
    unsafe {
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        let title = if len > 0 {
            OsString::from_wide(&title_buf[..len as usize])
                .to_string_lossy()
                .to_string()
        } else {
            String::new()
        };
        let mut info = WINDOWINFO {
            cbSize: std::mem::size_of::<WINDOWINFO>() as u32,
            ..Default::default()
        };
        GetWindowInfo(hwnd, &mut info).ok();
        let rect = info.rcWindow;
        let width = (rect.right - rect.left).max(0) as u32;
        let height = (rect.bottom - rect.top).max(0) as u32;
        Ok(GameWindow {
            title,
            left: rect.left,
            top: rect.top,
            width,
            height,
            dpi: get_dpi_for_window(hwnd),
        })
    }
}

/// EnumWindows callback: collect all visible, non-minimized top-level windows.
unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // Skip invisible and minimized windows
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return BOOL(1);
    }

    // Skip tool windows
    let mut info = WINDOWINFO {
        cbSize: std::mem::size_of::<WINDOWINFO>() as u32,
        ..Default::default()
    };
    // Get window title (before tool-window filter — League Client Ux uses WS_EX_TOOLWINDOW)
    let mut title_buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut title_buf);
    if len == 0 {
        return BOOL(1);
    }
    let title = OsString::from_wide(&title_buf[..len as usize])
        .to_string_lossy()
        .to_string();

    // Skip empty titles and common non-game windows
    if title.is_empty() || title == "Default IME" || title == "MSCTFIME UI" {
        return BOOL(1);
    }

    if GetWindowInfo(hwnd, &mut info).is_ok() {
        let is_tool = (info.dwExStyle & WS_EX_TOOLWINDOW) != Default::default();
        if is_tool && !title_matches_game_client(&title) {
            return BOOL(1);
        }
    }

    // Get window rect via GetWindowInfo
    let rect = info.rcWindow;
    let width = (rect.right - rect.left) as u32;
    let height = (rect.bottom - rect.top) as u32;

    // Calculate DPI
    let dpi = get_dpi_for_window(hwnd);

    let window = GameWindow {
        title,
        left: rect.left,
        top: rect.top,
        width,
        height,
        dpi,
    };

    // Store in the shared vector
    let candidates = &*(lparam.0 as *const Mutex<Vec<GameWindow>>);
    if let Ok(mut vec) = candidates.lock() {
        vec.push(window);
    }

    BOOL(1)
}

fn title_matches_game_client(title: &str) -> bool {
    let t = title.to_lowercase();
    [
        "英雄联盟",
        "league of legends",
        "云顶",
        "teamfight tactics",
        "tft",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// Get DPI for a window (falls back to 96 if unavailable).
fn get_dpi_for_window(hwnd: HWND) -> u32 {
    unsafe {
        let hdc: HDC = GetDC(hwnd);
        if hdc.is_invalid() {
            return 96;
        }
        let dpi = GetDeviceCaps(hdc, LOGPIXELSX) as u32;
        let _ = ReleaseDC(hwnd, hdc);
        if dpi > 0 { dpi } else { 96 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_cn_patterns() {
        let config = WindowMatchConfig::default();
        assert!(config.title_patterns.iter().any(|p| p.contains("英雄联盟")));
        assert!(config
            .title_patterns
            .iter()
            .any(|p| p.contains("League of Legends")));
    }

    #[test]
    fn window_match_config_clone() {
        let config = WindowMatchConfig::default();
        let cloned = config.clone();
        assert_eq!(config.title_patterns, cloned.title_patterns);
    }
}
