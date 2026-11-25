use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub formatter: bool,
    pub hotwords: Vec<String>,
    pub hotkey: String,
    pub meter_sensitivity: u8,
    pub show_completion: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            formatter: true,
            hotwords: Vec::new(),
            hotkey: super::HOTKEY.into(),
            meter_sensitivity: 100,
            show_completion: true,
        }
    }
}

pub struct SettingsState {
    path: PathBuf,
    settings: Mutex<Settings>,
    hyprland: bool,
}

fn normalize(mut settings: Settings) -> Result<Settings, String> {
    let mut parts: Vec<_> = settings.hotkey.split('+').map(str::trim).collect();
    let key = parts.pop().unwrap_or("");
    let mut modifiers = Vec::new();
    for modifier in ["Ctrl", "Alt", "Shift", "Super"] {
        if let Some(index) = parts
            .iter()
            .position(|part| part.eq_ignore_ascii_case(modifier))
        {
            parts.remove(index);
            modifiers.push(modifier);
        }
    }
    if modifiers.is_empty()
        || !parts.is_empty()
        || key.len() != 1
        || !key.as_bytes()[0].is_ascii_alphanumeric()
    {
        return Err(
            "Choose one or more modifiers and a letter or number, such as Ctrl+Alt+R.".into(),
        );
    }
    let key = key.to_ascii_uppercase();
    modifiers.push(&key);
    settings.hotkey = modifiers.join("+");

    let mut hotwords = Vec::new();
    for word in settings.hotwords {
        let word = word.trim();
        if word.is_empty() || hotwords.iter().any(|existing: &String| existing == word) {
            continue;
        }
        if word.chars().count() > 80 || word.chars().any(char::is_control) {
            return Err("Hotwords must be 80 characters or fewer and on separate lines.".into());
        }
        hotwords.push(word.to_owned());
    }
    if hotwords.len() > 25 {
        return Err("Phonon supports up to 25 hotwords.".into());
    }
    settings.hotwords = hotwords;
    settings.meter_sensitivity = settings.meter_sensitivity.clamp(50, 200);
    Ok(settings)
}

fn native_shortcut(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(hotkey, |app, _, event| {
            if event.state == ShortcutState::Pressed {
                super::trigger_hotkey(app);
            }
        })
        .map_err(|err| err.to_string())
}

pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path().app_config_dir()?.join("settings.json");
    let settings = fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .and_then(|value| normalize(value).ok())
        .unwrap_or_default();
    #[cfg(target_os = "linux")]
    let hyprland = if super::hyprland::active() {
        super::hyprland::setup(app.clone())?;
        true
    } else {
        false
    };
    #[cfg(not(target_os = "linux"))]
    let hyprland = false;
    if !hyprland {
        native_shortcut(app, &settings.hotkey)?;
    }
    app.manage(SettingsState {
        path,
        settings: Mutex::new(settings),
        hyprland,
    });
    Ok(())
}

pub fn snapshot(app: &AppHandle) -> Result<Settings, String> {
    app.state::<SettingsState>()
        .settings
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| "Settings are unavailable.".into())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    snapshot(&app)
}

#[tauri::command]
pub fn external_shortcut_command(app: AppHandle) -> Result<Option<String>, String> {
    if !app.state::<SettingsState>().hyprland {
        return Ok(None);
    }
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let exe = exe.to_str().ok_or("VType executable path is not UTF-8.")?;
    Ok(Some(format!("'{}' --toggle", exe.replace('\'', "'\\''"))))
}

fn rebind(app: &AppHandle, state: &SettingsState, old: &str, new: &str) -> Result<(), String> {
    if state.hyprland {
        return Ok(());
    }
    native_shortcut(app, new)?;
    if let Err(err) = app.global_shortcut().unregister(old) {
        let _ = app.global_shortcut().unregister(new);
        return Err(err.to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let next = normalize(settings)?;
        let state = app.state::<SettingsState>();
        let mut current = state
            .settings
            .lock()
            .map_err(|_| "Settings are unavailable.")?;
        let directory = state
            .path
            .parent()
            .ok_or("Settings directory is unavailable.")?;
        fs::create_dir_all(directory).map_err(|err| err.to_string())?;
        let pending = state.path.with_extension("tmp");
        let data = serde_json::to_vec_pretty(&next).map_err(|err| err.to_string())?;
        fs::write(&pending, data).map_err(|err| err.to_string())?;

        let changed = next.hotkey != current.hotkey;
        if changed {
            rebind(&app, &state, &current.hotkey, &next.hotkey)?;
        }
        if let Err(err) = fs::rename(&pending, &state.path) {
            if changed {
                rebind(&app, &state, &next.hotkey, &current.hotkey)?;
            }
            return Err(err.to_string());
        }
        *current = next.clone();
        drop(current);
        let _ = app.emit("settings-changed", &next);
        Ok(next)
    })
    .await
    .map_err(|err| err.to_string())?
}
