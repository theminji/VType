use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    sync::Mutex,
};
use tauri::{AppHandle, Emitter, Manager};

pub struct Setup {
    marker: Option<PathBuf>,
    needed: AtomicBool,
    ready: AtomicBool,
    installing: Mutex<()>,
}

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let marker = if super::runtime::bundled_uv(app)?.is_some() {
        Some(
            app.path()
                .app_local_data_dir()
                .map_err(|err| err.to_string())?
                .join("setup-v2.complete"),
        )
    } else {
        None
    };
    let needed = marker.as_ref().is_some_and(|path| !path.is_file());
    app.manage(Setup {
        marker,
        needed: AtomicBool::new(needed),
        ready: AtomicBool::new(false),
        installing: Mutex::new(()),
    });
    if needed {
        show(app)?;
    }
    Ok(())
}

pub fn needed(app: &AppHandle) -> bool {
    app.try_state::<Setup>()
        .is_some_and(|state| state.needed.load(Ordering::Acquire))
}

pub fn show(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("onboarding")
        .ok_or("Setup window is unavailable.")?;
    window
        .show()
        .and_then(|_| window.set_focus())
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn install_components(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Setup>();
        let _install = state
            .installing
            .try_lock()
            .map_err(|_| "Setup is already running.")?;
        if state.ready.load(Ordering::Acquire) {
            return Ok(());
        }
        let install = || {
            let _ = app.emit_to("onboarding", "setup-stage", "runtime");
            if std::env::var_os("VTYPE_PYTHON").is_none() {
                super::runtime::bundled_python(&app)?;
            }
            let _ = app.emit_to("onboarding", "setup-stage", "speech");
            super::ensure_worker(&app)?;
            let _ = app.emit_to("onboarding", "setup-stage", "formatter");
            super::formatter::prepare(&app)?;
            Ok::<(), String>(())
        };
        if let Err(error) = install() {
            let _ = super::log_message(format!("Setup failed: {error}"));
            return Err(error);
        }
        state.ready.store(true, Ordering::Release);
        Ok(())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn finish_setup(app: AppHandle) -> Result<(), String> {
    let state = app.state::<Setup>();
    if !state.ready.load(Ordering::Acquire) {
        return Err("Finish installing the models first.".into());
    }
    if let Some(path) = &state.marker {
        let directory = path.parent().ok_or("Setup directory is unavailable.")?;
        fs::create_dir_all(directory).map_err(|err| err.to_string())?;
        fs::write(path, b"complete\n").map_err(|err| err.to_string())?;
    }
    state.needed.store(false, Ordering::Release);
    if let Some(window) = app.get_webview_window("onboarding") {
        window.hide().map_err(|err| err.to_string())?;
    }
    Ok(())
}
