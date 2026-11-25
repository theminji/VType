#[cfg(target_os = "linux")]
mod hyprland;
mod runtime;
mod settings;

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

#[cfg(target_os = "macos")]
const HOTKEY: &str = "Alt+Super+R";
#[cfg(not(target_os = "macos"))]
const HOTKEY: &str = "Ctrl+Alt+R";
const TRAY_MENU_SHOW: &str = "tray_show";
const TRAY_MENU_SETTINGS: &str = "tray_settings";
const TRAY_MENU_QUIT: &str = "tray_quit";

#[tauri::command]
async fn transcribe_wav(app: tauri::AppHandle, wav_base64: String) -> Result<String, String> {
    let _ = log_message(format!(
        "Transcribe request received, bytes(base64)={}",
        wav_base64.len()
    ));
    tauri::async_runtime::spawn_blocking(move || {
        let wav_bytes = general_purpose::STANDARD
            .decode(wav_base64)
            .map_err(|err| err.to_string())?;

        let hotwords = settings::snapshot(&app)?.hotwords;
        let terms = serde_json::to_vec(&hotwords).map_err(|err| err.to_string())?;
        let mut payload = Vec::with_capacity(4 + terms.len() + wav_bytes.len());
        payload.extend_from_slice(&(terms.len() as u32).to_le_bytes());
        payload.extend_from_slice(&terms);
        payload.extend_from_slice(&wav_bytes);
        let response = with_worker(&app, |worker| send_request(worker, b'T', &payload))?;
        let text = response.trim().to_string();
        let _ = log_message(format!("Transcribe success, chars={}", text.len()));
        Ok(text)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn cleanup_transcription(
    app: tauri::AppHandle,
    text: String,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cleaned = with_worker(&app, |worker| send_request(worker, b'C', text.as_bytes()))?;
        Ok((!cleaned.is_empty()).then_some(cleaned))
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn insert_transcription(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Typing a line break or tab can submit a command or move focus.
        let text: String = text
            .chars()
            .filter_map(|character| {
                if character.is_control() {
                    return character.is_whitespace().then_some(' ');
                }
                Some(character)
            })
            .collect();
        type_text(&text)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn type_text(text: &str) -> Result<(), String> {
    use enigo::{Enigo, Keyboard, Settings};

    let settings = Settings::default();
    #[cfg(target_os = "linux")]
    let settings = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        // Enigo sends to every connected backend. Use only Wayland in Wayland sessions.
        Settings {
            x11_display: Some(String::new()),
            ..settings
        }
    } else {
        Settings {
            wayland_display: Some("/dev/null".into()),
            ..settings
        }
    };

    let mut keyboard = Enigo::new(&settings).map_err(|err| err.to_string())?;
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        // Keyboard events work in native text fields and apps such as terminals.
        use enigo::{Direction, Key};
        return text
            .chars()
            .try_for_each(|character| keyboard.key(Key::Unicode(character), Direction::Click))
            .map_err(|err| err.to_string());
    }
    keyboard.text(text).map_err(|err| err.to_string())
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
async fn warm_asr(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || ensure_worker(&app))
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

fn ensure_worker(app: &tauri::AppHandle) -> Result<(), String> {
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
        *guard = Some(start_worker(app)?);
    }
    Ok(())
}

fn with_worker<F>(app: &tauri::AppHandle, mut f: F) -> Result<String, String>
where
    F: FnMut(&mut AsrWorker) -> Result<String, String>,
{
    ensure_worker(app)?;
    let mut guard = worker_state()
        .lock()
        .map_err(|_| "Worker lock poisoned".to_string())?;
    if guard.is_none() {
        return Err("ASR worker not available".to_string());
    }
    let worker = guard.as_mut().unwrap();
    f(worker)
}

fn start_worker(app: &tauri::AppHandle) -> Result<AsrWorker, String> {
    let mut worker_cmd =
        if let Some(python) = std::env::var_os("VTYPE_PYTHON").or_else(project_python) {
            Command::new(python)
        } else if let Some(command) = runtime::bundled_python(app)? {
            command
        } else {
            Command::new(
                resolve_python().ok_or("Python interpreter not found (tried python3, python)")?,
            )
        };
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

fn project_python() -> Option<std::ffi::OsString> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let dir = std::env::current_dir().ok()?;
    let relative = if cfg!(windows) {
        ".venv/Scripts/python.exe"
    } else {
        ".venv/bin/python"
    };
    std::iter::once(dir.as_path())
        .chain(dir.parent())
        .map(|dir| dir.join(relative))
        .find(|python| python.is_file())
        .map(|python| python.into_os_string())
}

fn resolve_python() -> Option<std::ffi::OsString> {
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

#[tauri::command]
fn open_settings(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("Settings window is unavailable.")?;
    window
        .show()
        .and_then(|_| window.set_focus())
        .map_err(|err| err.to_string())
}

fn trigger_hotkey(app: &tauri::AppHandle) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = app_handle.get_webview_window("settings") {
            if window.is_visible().unwrap_or(false) {
                let _ = window.set_focus();
                return;
            }
        }
        show_main_window(&app_handle);
        let _ = app_handle.emit("hotkey-pressed", ());
    });
}

#[cfg(target_os = "linux")]
pub fn toggle() -> Result<(), String> {
    hyprland::toggle()
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
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "settings" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
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
                .text(TRAY_MENU_SETTINGS, "Settings")
                .separator()
                .text(TRAY_MENU_QUIT, "Quit")
                .build()?;

            let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                .menu(&tray_menu)
                .tooltip("VType")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    TRAY_MENU_SHOW => show_ready_overlay(app),
                    TRAY_MENU_SETTINGS => {
                        let _ = app.emit("open-settings", ());
                    }
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

            settings::setup(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            transcribe_wav,
            cleanup_transcription,
            insert_transcription,
            log_message,
            save_wav_temp,
            warm_asr,
            hide_overlay,
            open_settings,
            settings::get_settings,
            settings::external_shortcut_command,
            settings::save_settings
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
