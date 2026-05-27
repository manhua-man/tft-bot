use std::thread;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Bring a window to foreground by title substring.
fn activate_window(title_sub: &str) -> anyhow::Result<()> {
    unsafe {
        // Find window by enumerating
        let target: Vec<u16> = title_sub.encode_utf16().chain(std::iter::once(0)).collect();
        let hwnd = FindWindowW(None, windows::core::PCWSTR(target.as_ptr()));
        match hwnd {
            Ok(h) if !h.is_invalid() => {
                ShowWindow(h, SW_RESTORE);
                SetForegroundWindow(h);
                thread::sleep(Duration::from_millis(500));
                Ok(())
            }
            _ => Err(anyhow::anyhow!("Window '{}' not found", title_sub)),
        }
    }
}

/// Send a virtual key press and release.
fn send_key(vk: VIRTUAL_KEY) -> anyhow::Result<()> {
    unsafe {
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: 0,
                        dwFlags: KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
    Ok(())
}

/// Send a mouse click at absolute screen position.
fn click_at(x: i32, y: i32) -> anyhow::Result<()> {
    unsafe {
        let (screen_w, screen_h): (u32, u32) = (
            GetSystemMetrics(SM_CXSCREEN) as u32,
            GetSystemMetrics(SM_CYSCREEN) as u32,
        );
        let abs_x = (x as f32 / screen_w as f32 * 65535.0) as i32;
        let abs_y = (y as f32 / screen_h as f32 * 65535.0) as i32;

        let inputs = [
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: abs_x,
                        dy: abs_y,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_LEFTDOWN,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_LEFTUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("activate") => {
            let title = args.get(2).ok_or_else(|| anyhow::anyhow!("Usage: queue-helper activate <title>"))?;
            activate_window(title)?;
            println!("Activated: {}", title);
        }
        Some("key") => {
            let key = args.get(2).ok_or_else(|| anyhow::anyhow!("Usage: queue-helper key <VK_CODE>"))?;
            let vk = key.parse::<u16>().map_err(|_| anyhow::anyhow!("Invalid VK code"))?;
            send_key(VIRTUAL_KEY(vk))?;
            println!("Sent key: {}", vk);
        }
        Some("click") => {
            let x: i32 = args.get(2).ok_or_else(|| anyhow::anyhow!("Usage: queue-helper click X Y"))?.parse()?;
            let y: i32 = args.get(3).ok_or_else(|| anyhow::anyhow!("Usage: queue-helper click X Y"))?.parse()?;
            click_at(x, y)?;
            println!("Clicked at ({}, {})", x, y);
        }
        Some("enter") => {
            send_key(VK_RETURN)?;
            println!("Sent Enter");
        }
        Some("escape") => {
            send_key(VK_ESCAPE)?;
            println!("Sent Escape");
        }
        _ => {
            eprintln!("Usage: queue-helper <activate|key|click|enter|escape> [args]");
        }
    }

    Ok(())
}
