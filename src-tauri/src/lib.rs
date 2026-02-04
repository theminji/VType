use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose;
use base64::Engine as _;
use tauri::{Emitter, Manager, Position};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[cfg(target_os = "macos")]
const HOTKEY: &str = "Command+Option+R";
#[cfg(not(target_os = "macos"))]
const HOTKEY: &str = "Ctrl+Alt+R";

#[tauri::command]
async fn transcribe_wav(wav_base64: String) -> Result<String, String> {
    let _ = log_message(format!("Transcribe request received, bytes(base64)={}", wav_base64.len()));
    tauri::async_runtime::spawn_blocking(move || {
        let wav_bytes = general_purpose::STANDARD
            .decode(wav_base64)
            .map_err(|err| err.to_string())?;

        let response = with_worker(|worker| send_wav(worker, &wav_bytes))?;
        let text = response.trim().to_string();
        let _ = log_message(format!("Transcribe success, chars={}", text.len()));
    Ok(text)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
fn paste_transcription(app: tauri::AppHandle, text: String) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|err| err.to_string())?;

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    std::thread::sleep(std::time::Duration::from_millis(200));

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
fn warm_asr() -> Result<(), String> {
    std::thread::spawn(|| {
        let _ = ensure_worker();
    });
    Ok(())
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
    let mut guard = worker_state().lock().map_err(|_| "Worker lock poisoned".to_string())?;
    let needs_start = match guard.as_mut() {
        Some(worker) => worker.child.try_wait().map_err(|err| err.to_string())?.is_some(),
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
    let mut guard = worker_state().lock().map_err(|_| "Worker lock poisoned".to_string())?;
    if guard.is_none() {
        return Err("ASR worker not available".to_string());
    }
    let worker = guard.as_mut().unwrap();
    f(worker)
}

fn start_worker() -> Result<AsrWorker, String> {
    let script_path = resolve_script_path().ok_or("transcribe_wav.py not found")?;
    let python = resolve_python().ok_or("Python interpreter not found (tried python3, python)")?;

    let mut child = Command::new(python)
        .arg(script_path)
        .arg("--worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| err.to_string())?;

    let stdin = child.stdin.take().ok_or("Failed to open ASR stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to open ASR stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut ready = String::new();
    reader
        .read_line(&mut ready)
        .map_err(|err| err.to_string())?;
    if ready.trim() != "ready" {
        return Err(format!("ASR worker not ready: {}", ready.trim()));
    }

    Ok(AsrWorker {
        child,
        stdin,
        stdout: reader,
    })
}

fn send_wav(worker: &mut AsrWorker, wav_bytes: &[u8]) -> Result<String, String> {
    let len = u32::try_from(wav_bytes.len()).map_err(|_| "WAV too large".to_string())?;
    worker
        .stdin
        .write_all(&len.to_le_bytes())
        .map_err(|err| err.to_string())?;
    worker
        .stdin
        .write_all(wav_bytes)
        .map_err(|err| err.to_string())?;
    worker.stdin.flush().map_err(|err| err.to_string())?;

    let mut header = [0u8; 4];
    worker
        .stdout
        .read_exact(&mut header)
        .map_err(|err| err.to_string())?;
    let resp_len = u32::from_le_bytes(header) as usize;
    let mut buf = vec![0u8; resp_len];
    if resp_len > 0 {
        worker
            .stdout
            .read_exact(&mut buf)
            .map_err(|err| err.to_string())?;
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    if text.starts_with("ERROR:") {
        return Err(text);
    }
    Ok(text)
}

fn resolve_script_path() -> Option<PathBuf> {
    let current = std::env::current_dir().ok()?;
    let candidates = [
        current.join("transcribe_wav.py"),
        current.join("src-tauri").join("transcribe_wav.py"),
    ];

    candidates.into_iter().find(|path| path.exists())
}

fn resolve_python() -> Option<&'static str> {
    let candidates = ["python3", "python"];
    for candidate in candidates {
        if Command::new(candidate)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return Some(candidate);
        }
    }
    None
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

            let handle = app.handle();
            handle
                .global_shortcut()
                .on_shortcut(HOTKEY, move |app, _shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let app_handle = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        if let Some(window) = app_handle.get_webview_window("main") {
                            if let Ok(monitor) = window.current_monitor() {
                                if let Some(monitor) = monitor {
                                    if let Ok(size) = window.outer_size() {
                                        let monitor_size = monitor.size();
                                        let x =
                                            (monitor_size.width.saturating_sub(size.width) / 2)
                                                as i32;
                                        let y = monitor_size
                                            .height
                                            .saturating_sub(size.height + 24)
                                            as i32;
                                        let _ =
                                            window.set_position(Position::Physical((x, y).into()));
                                    }
                                }
                            }
                            let _ = window.show();
                            let _ = window.set_focusable(false);
                        }
                        let _ = app_handle.emit("hotkey-pressed", ());
                    });
                })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            transcribe_wav,
            paste_transcription,
            log_message,
            save_wav_temp,
            warm_asr
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
