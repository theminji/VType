#[cfg(target_os = "linux")]
mod hyprland;

use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose;
use base64::Engine as _;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, Position};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[cfg(target_os = "macos")]
const HOTKEY: &str = "Command+Option+R";
#[cfg(not(target_os = "macos"))]
const HOTKEY: &str = "Ctrl+Alt+R";
const TRAY_MENU_SHOW: &str = "tray_show";
const TRAY_MENU_QUIT: &str = "tray_quit";

#[tauri::command]
async fn transcribe_wav(wav_base64: String) -> Result<String, String> {
    let _ = log_message(format!(
        "Transcribe request received, bytes(base64)={}",
        wav_base64.len()
    ));
    tauri::async_runtime::spawn_blocking(move || {
        let wav_bytes = general_purpose::STANDARD
            .decode(wav_base64)
            .map_err(|err| err.to_string())?;

        let response = with_worker(|worker| send_request(worker, b'T', &wav_bytes))?;
        let text = response.trim().to_string();
        let _ = log_message(format!("Transcribe success, chars={}", text.len()));
        Ok(text)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn cleanup_transcription(text: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cleaned = with_worker(|worker| send_request(worker, b'C', text.as_bytes()))?;
        Ok((!cleaned.is_empty()).then_some(cleaned))
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn paste_transcription(app: tauri::AppHandle, text: String) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|err| err.to_string())?;

    tauri::async_runtime::spawn_blocking(paste_into_active_app)
        .await
        .map_err(|err| err.to_string())?
}

fn paste_into_active_app() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use enigo::{Key, KeyboardControllable};
        paste_with_retry(|| {
            let mut enigo = enigo::Enigo::new();
            enigo.key_down(Key::Meta);
            enigo.key_click(Key::Layout('v'));
            enigo.key_up(Key::Meta);
            Ok(())
        })?;
    }
    #[cfg(target_os = "windows")]
    {
        use enigo::{Key, KeyboardControllable};
        paste_with_retry(|| {
            let mut enigo = enigo::Enigo::new();
            enigo.key_down(Key::Control);
            enigo.key_click(Key::Layout('v'));
            enigo.key_up(Key::Control);
            Ok(())
        })?;
    }
    #[cfg(target_os = "linux")]
    {
        paste_with_retry(|| linux_paste().map_err(|err| err.to_string()))?;
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_paste() -> Result<(), String> {
    if hyprland::active() {
        return hyprland::paste();
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        // Wayland: use wtype if available.
        let status = Command::new("wtype")
            .args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])
            .status()
            .map_err(|err| err.to_string())?;
        if status.success() {
            return Ok(());
        }
        return Err("wtype failed to paste on Wayland".to_string());
    }

    // X11: use xdotool if available.
    let status = Command::new("xdotool")
        .args(["key", "--clearmodifiers", "ctrl+v"])
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("xdotool failed to paste on X11".to_string())
    }
}

fn paste_with_retry<F>(mut paste_fn: F) -> Result<(), String>
where
    F: FnMut() -> Result<(), String>,
{
    if let Err(first_err) = paste_fn() {
        std::thread::sleep(std::time::Duration::from_millis(220));
        if let Err(second_err) = paste_fn() {
            return Err(format!("paste failed: {} | {}", first_err, second_err));
        }
    }
    Ok(())
}

#[tauri::command]
fn log_message(message: String) -> Result<(), String> {
    let mut path = std::env::temp_dir();
    path.push("vtype.log");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("[{}] {}\n", timestamp, message);
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn save_wav_temp(wav_base64: String) -> Result<String, String> {
    let wav_bytes = general_purpose::STANDARD
        .decode(wav_base64)
        .map_err(|err| err.to_string())?;
    let path = std::env::temp_dir().join("vtype_last.wav");
    fs::write(&path, wav_bytes).map_err(|err| err.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn warm_asr() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(ensure_worker)
        .await
        .map_err(|err| err.to_string())?
}

struct AsrWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

static ASR_WORKER: OnceLock<Mutex<Option<AsrWorker>>> = OnceLock::new();
fn worker_state() -> &'static Mutex<Option<AsrWorker>> {
    ASR_WORKER.get_or_init(|| Mutex::new(None))
}

fn ensure_worker() -> Result<(), String> {
    let mut guard = worker_state()
        .lock()
        .map_err(|_| "Worker lock poisoned".to_string())?;
    let needs_start = match guard.as_mut() {
        Some(worker) => worker
            .child
            .try_wait()
            .map_err(|err| err.to_string())?
            .is_some(),
        None => true,
    };
    if needs_start {
        *guard = Some(start_worker()?);
    }
    Ok(())
}

fn with_worker<F>(mut f: F) -> Result<String, String>
where
    F: FnMut(&mut AsrWorker) -> Result<String, String>,
{
    ensure_worker()?;
    let mut guard = worker_state()
        .lock()
        .map_err(|_| "Worker lock poisoned".to_string())?;
    if guard.is_none() {
        return Err("ASR worker not available".to_string());
    }
    let worker = guard.as_mut().unwrap();
    f(worker)
}

fn start_worker() -> Result<AsrWorker, String> {
    let python = resolve_python().ok_or("Python interpreter not found (tried python3, python)")?;

    let mut worker_cmd = Command::new(python);
    worker_cmd
        .arg("-c")
        .arg(include_str!("../transcribe_wav.py"))
        .arg("--worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    configure_background_command(&mut worker_cmd);
    let mut child = worker_cmd.spawn().map_err(|err| err.to_string())?;

    let stdin = child.stdin.take().ok_or("Failed to open ASR stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to open ASR stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut ready = String::new();
    reader
        .read_line(&mut ready)
        .map_err(|err| err.to_string())?;
    if ready.trim() != "ready" {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("Phonon worker could not start. {}", ready.trim()));
    }

    Ok(AsrWorker {
        child,
        stdin,
        stdout: reader,
    })
}

fn send_request(worker: &mut AsrWorker, operation: u8, payload: &[u8]) -> Result<String, String> {
    let len = u32::try_from(payload.len()).map_err(|_| "Speech request too large".to_string())?;
    worker
        .stdin
        .write_all(&[operation])
        .map_err(|err| err.to_string())?;
    worker
        .stdin
        .write_all(&len.to_le_bytes())
        .map_err(|err| err.to_string())?;
    worker
        .stdin
        .write_all(payload)
        .map_err(|err| err.to_string())?;
    worker.stdin.flush().map_err(|err| err.to_string())?;

    let mut header = [0u8; 5];
    worker
        .stdout
        .read_exact(&mut header)
        .map_err(|err| err.to_string())?;
    let resp_len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
    let mut buf = vec![0u8; resp_len];
    if resp_len > 0 {
        worker
            .stdout
            .read_exact(&mut buf)
            .map_err(|err| err.to_string())?;
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    if header[0] != 0 {
        return Err(text);
    }
    Ok(text)
}

fn resolve_python() -> Option<std::ffi::OsString> {
    if let Some(python) = std::env::var_os("VTYPE_PYTHON") {
        return Some(python);
    }
    if let Ok(dir) = std::env::current_dir() {
        let relative = if cfg!(windows) {
            ".venv/Scripts/python.exe"
        } else {
            ".venv/bin/python"
        };
        for dir in std::iter::once(dir.as_path()).chain(dir.parent()) {
            let python = dir.join(relative);
            if python.is_file() {
                return Some(python.into_os_string());
            }
        }
    }
    for candidate in ["python3", "python"] {
        let mut version_cmd = Command::new(candidate);
        version_cmd
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_command(&mut version_cmd);
        if version_cmd.status().is_ok_and(|status| status.success()) {
            return Some(candidate.into());
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn configure_background_command(command: &mut Command) {
    // CREATE_NO_WINDOW
    command.creation_flags(0x08000000);
}

#[cfg(not(target_os = "windows"))]
fn configure_background_command(_command: &mut Command) {}

#[tauri::command]
fn hide_overlay(window: tauri::WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|err| err.to_string())
}

fn trigger_hotkey(app: &tauri::AppHandle) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        show_main_window(&app_handle);
        let _ = app_handle.emit("hotkey-pressed", ());
    });
}

#[cfg(target_os = "linux")]
pub fn toggle(pid: u32) -> Result<(), String> {
    hyprland::toggle(pid)
}

fn show_ready_overlay(app: &tauri::AppHandle) {
    show_main_window(app);
    let _ = app.emit("show-overlay", ());
}

fn show_main_window(app_handle: &tauri::AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        #[cfg(target_os = "windows")]
        {
            let _ = window.set_shadow(false);
        }
        if let Ok(monitor) = window.current_monitor() {
            if let Some(monitor) = monitor {
                if let Ok(size) = window.outer_size() {
                    let monitor_size = monitor.size();
                    let x = (monitor_size.width.saturating_sub(size.width) / 2) as i32;
                    let y = monitor_size.height.saturating_sub(size.height + 24) as i32;
                    let _ = window.set_position(Position::Physical((x, y).into()));
                }
            }
        }
        let _ = window.set_focusable(false);
        let _ = window.show();
        let _ = window.set_ignore_cursor_events(true);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "linux")]
            {
                use webkit2gtk::{PermissionRequestExt, SettingsExt, WebViewExt};

                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.with_webview(|webview| {
                        let inner = webview.inner();

                        // Allow mic access without portal prompts (temporary workaround).
                        inner.connect_permission_request(|_, request| {
                            request.allow();
                            true
                        });

                        if let Some(settings) = inner.settings() {
                            settings.set_enable_media_stream(true);
                        }
                    });
                    let _ = window.set_focusable(false);
                }
            }
            #[cfg(target_os = "windows")]
            {
                if let Some(window) = app.get_webview_window("main") {
                    // Prevent the DWM frame/shadow artifact around transparent windows.
                    let _ = window.set_shadow(false);
                }
            }
            let tray_menu = MenuBuilder::new(app)
                .text(TRAY_MENU_SHOW, "Show VType")
                .separator()
                .text(TRAY_MENU_QUIT, "Quit")
                .build()?;

            let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                .menu(&tray_menu)
                .tooltip("VType")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    TRAY_MENU_SHOW => show_ready_overlay(app),
                    TRAY_MENU_QUIT => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_ready_overlay(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
            let _ = tray_builder.build(app)?;

            let handle = app.handle();
            #[cfg(target_os = "linux")]
            if hyprland::active() {
                match hyprland::setup(handle.clone()) {
                    Ok(()) => return Ok(()),
                    Err(err) => eprintln!("VType Hyprland integration: {err}"),
                }
            }

            handle
                .global_shortcut()
                .on_shortcut(HOTKEY, move |app, _shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    trigger_hotkey(app);
                })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            transcribe_wav,
            cleanup_transcription,
            paste_transcription,
            log_message,
            save_wav_temp,
            warm_asr,
            hide_overlay
        ])
        .build(tauri::generate_context!())
        .expect("error while building VType")
        .run(|_app, event| {
            #[cfg(target_os = "linux")]
            if matches!(event, tauri::RunEvent::Exit) {
                hyprland::cleanup();
            }
            #[cfg(not(target_os = "linux"))]
            let _ = event;
        });
}
